use std::iter::once;
use proc_macro2::{TokenStream, TokenTree};
use quote::{quote, format_ident};
use syn::{
    parse::{Parse, ParseStream},
    parse2,
    punctuated::Punctuated,
    Expr, Ident, Token, Result,
};
use syn::Block;

/// Represents a single task mapping within an executor.
///
/// Syntax: `frequency_expression -> function_expression`
///
/// The `freq` field supports arbitrary Rust expressions that resolve to a frequency
/// (typically `u64`), while `func` is the closure or function to be executed.
struct TaskEntry {
    freq: Expr,
    func: Expr,
}

impl Parse for TaskEntry {
    /// Parses a `TaskEntry` by greedily collecting tokens until a `->` token is encountered.
    /// This allows for complex expressions (like math or function calls) on the left-hand side.
    fn parse(input: ParseStream) -> Result<Self> {
        // its hacky, but it works
        // Collect tokens for the frequency expression until the arrow separator
        let mut tokens = TokenStream::new();
        while !input.is_empty() && !input.peek(Token![->]) {
            tokens.extend(once(input.parse::<TokenTree>()?));
        }

        // parse left side
        let freq: Expr = parse2(tokens)?;

        // parse -> side
        let _: Token![->] = input.parse()?;

        // parse right side
        let func: Expr = input.parse()?;

        Ok(TaskEntry { freq, func })
    }
}

/// Represents an executor and its associated collection of tasks.
///
/// Syntax: `executor_name => { task1, task2, ... }`
struct ExecutorEntry {
    /// The identifier of the executor instance.
    name: Ident,
    /// A comma-separated list of tasks assigned to this executor.
    tasks: Punctuated<TaskEntry, Token![,]>,
}

impl Parse for ExecutorEntry {
    fn parse(input: ParseStream) -> Result<Self> {
        let name: Ident = input.parse()?;
        let _: Token![=>] = input.parse()?;
        let content;
        syn::braced!(content in input);
        let tasks = content.parse_terminated(TaskEntry::parse, Token![,])?;
        Ok(ExecutorEntry { name, tasks })
    }
}

/// The root input structure for the macro, containing multiple executor definitions.
struct AddInput(Punctuated<ExecutorEntry, Token![,]>);

impl Parse for AddInput {
    fn parse(input: ParseStream) -> Result<Self> {
        Ok(AddInput(input.parse_terminated(ExecutorEntry::parse, Token![,])?))
    }
}

/// Procedural macro logic to initialize and register task nodes to executors.
///
/// # Expansion
/// For every task defined, this macro generates:
/// 1. A unique variable name (e.g., `__node_0_1`).
/// 2. A `TaskNode` initialization with a pinned function and frequency.
/// 3. A registration call adding the node to the specified executor.
///
/// # Example Input
/// ```rust, no_run
/// executor => {
///     100 -> task_one(),
///     my_config.interval -> || { do_work() }
/// }
/// ```
pub fn task(input: TokenStream) -> TokenStream {
    let input = match parse2::<AddInput>(input) {
        Ok(val) => val,
        Err(err) => return err.to_compile_error(),
    };

    let mut declarations = Vec::new();

    for (exec_idx, exec_entry) in input.0.iter().enumerate() {
        let exec_name = &exec_entry.name;

        for (task_idx, task) in exec_entry.tasks.iter().enumerate() {
            let var_name = format_ident!("__node_{}_{}", exec_idx, task_idx);
            let func = &task.func;
            let freq = &task.freq;

            declarations.push(quote! {
                let mut #var_name = TaskNode::new(Box::pin(#func), #freq);
                #exec_name.add(&mut #var_name);
            })
        }
    }

    quote! { #(#declarations)* }
}

pub mod join {
    use super::*;

    fn get_path() -> TokenStream { quote! { ::uefi_async::nano_alloc::control::single::join } }


    struct JoinInput (Punctuated<Expr, Token![,]>);

    impl Parse for JoinInput {
        fn parse(input: ParseStream) -> Result<Self> {
            let exprs = Punctuated::parse_terminated(input)?;
            Ok(JoinInput(exprs))
        }
    }

    /// Joins multiple futures into a single future that runs them concurrently.
    ///
    /// This macro creates a nested `Join` or `TryJoin` structure at compile-time.
    /// It is designed for futures that return `()` (for `join!`) or `Result<(), E>` (for `try_join!`).
    ///
    /// # Parameters
    /// - `input`: A comma-separated list of future expressions.
    /// - `is_try`: If true, uses `TryJoin` logic (short-circuits on error).
    ///
    /// # Behavior
    /// 1. **Concurrency**: All passed futures are polled within the same executor step.
    /// 2. **Completion**:
    ///    - For `join!`, it resolves when **all** futures have resolved to `()`.
    ///    - For `try_join!`, it resolves to `Err` as soon as any future fails, or `Ok(())` when all succeed.
    /// 3. **Memory**: No heap allocation. The nested structure is stored in the task's stack frame.
    ///
    /// # Example
    /// ```rust
    /// // Concurrent side-effects
    /// join!(print_heartbeat(), update_cursor()).await;
    ///
    /// // Fallible concurrent operations
    /// try_join!(init_pci_bus(), load_kernel_file()).await?;
    /// ```
    pub fn join(input: TokenStream, is_try: bool) -> TokenStream {
        let input = match parse2::<JoinInput>(input) {
            Ok(res) => res,
            Err(err) => return err.to_compile_error(),
        };

        let mut expr_list: Vec<Expr> = input.0.into_iter().collect();

        if expr_list.is_empty() { return quote! { async {} } }

        let tree = build_join_tree(&mut expr_list, is_try);

        quote! { async move { #tree.await } }
    }

    /// Internal helper to recursively construct the `Join` or `TryJoin` tree.
    ///
    /// This ensures that the complexity of the future is handled by the compiler's
    /// type system rather than a runtime list, maintaining Zero-Cost Abstraction.
    fn build_join_tree(exprs: &mut Vec<Expr>, is_try: bool) -> TokenStream {
        if exprs.len() == 1 {
            let last = exprs.pop().unwrap();
            return quote! { #last }
        }

        let head = exprs.remove(0);
        let tail = build_join_tree(exprs, is_try);

        let path = get_path();

        let class = if is_try { quote! { #path::TryJoin } }
        else { quote! { #path::Join } };

        quote! { #class { head: #head, tail: #tail, head_done: false, tail_done: false } }
    }

    /// Joins multiple futures and collects their heterogeneous output into a flattened tuple.
    ///
    /// This macro solves the problem of awaiting multiple tasks that return different types.
    /// It transforms a recursive `JoinAll` tree into a clean, flat tuple using pattern destructuring.
    ///
    /// # Constraints
    /// All futures passed must implement `core::future::Future`.
    ///
    /// # Output
    /// Returns a tuple `(T0, T1, T2, ...)` where `Tn` is the `Output` type of the `n-th` future.
    ///
    /// # Internal Mechanism
    ///
    /// The macro builds a nested structure like `JoinAll<F0, JoinAll<F1, F2>>`. Upon completion,
    /// it generates a pattern match to extract values from the nested results:
    /// `let (r0, (r1, r2)) = tree.await;` and returns `(r0, r1, r2)`.
    ///
    /// # Example
    /// ```rust
    /// let (video_info, buffer_ptr) = join_all!(get_gop_mode(), allocate_pool(size)).await;
    /// // video_info is a ModeInfo, buffer_ptr is a *mut u8
    /// ```
    pub fn join_all(input: TokenStream) -> TokenStream {
        let input = match parse2::<JoinInput>(input) {
            Ok(res) => res,
            Err(err) => return err.to_compile_error(),
        };

        let exprs: Vec<Expr> = input.0.into_iter().collect();
        let count = exprs.len();

        if count == 0 {  return quote! { async { () } } }
        if count == 1 {
            let f = &exprs[0];
            return quote! { async move { #f.await } };
        }

        let tree = build_join_all_tree(&exprs);

        let res_idents: Vec<Ident> = (0..count)
            .map(|i| format_ident!("__res_{}", i))
            .collect();

        let mut pattern = {
            let last_ident = &res_idents[count - 1];
            quote! { #last_ident }
        };

        for i in (0..count - 1).rev() {
            let id = &res_idents[i];
            let prev_pattern = pattern;
            pattern = quote! { (#id, #prev_pattern) };
        }

        quote! { async move { let #pattern = #tree.await; ( #(#res_idents),* ) } }
    }

    /// Internal helper to recursively construct the `JoinAll` tree.
    ///
    /// This specifically handles the preservation of return types for each branch
    /// by nesting `JoinAll` structs until a base case of two futures is reached.
    fn build_join_all_tree(exprs: &[Expr]) -> TokenStream {
        let count = exprs.len();
        let head = &exprs[0];

        let path = get_path();

        if count == 2 {
            let tail = &exprs[1];
            return quote! { #path::JoinAll::new(#head, #tail) };
        }

        let tail_tree = build_join_all_tree(&exprs[1..]);

        quote! { #path::JoinAll::new(#head, #tail_tree) }
    }
}

/// Polls multiple futures and returns the result of the first one that completes.
pub mod select {
    use super::*;

    fn get_path() -> TokenStream { quote! { ::uefi_async::nano_alloc::control::single::select } }

    // Analyzing a single branch: expr => { block }
    struct SelectBranch { expr: Expr, _arrow: Token![=>], block: Block }

    impl Parse for SelectBranch {
        fn parse(input: ParseStream) -> Result<Self> {
            Ok(SelectBranch {
                expr: input.parse()?,
                _arrow: input.parse()?,
                block: input.parse()?,
            })
        }
    }

    // Analyze the entire macro input: select! { ... }
    struct SelectInput (Punctuated<SelectBranch, Token![,]>);

    impl Parse for SelectInput {
        fn parse(input: ParseStream) -> Result<Self> {
            Ok(SelectInput(Punctuated::parse_terminated(input)?))
        }
    }
    pub fn select(input: TokenStream) -> TokenStream {
        let input = match parse2::<SelectInput>(input) {
            Ok(res) => res,
            Err(err) => return err.to_compile_error(),
        };

        let branches: Vec<SelectBranch> = input.0.into_iter().collect();
        let count = branches.len();
        let path = get_path();

        if count == 0 { return quote! { core::future::pending::<()>().await } }

        // Extract all expressions to construct a Future tree
        let exprs: Vec<Expr> = branches.iter().map(|b| b.expr.clone()).collect();
        let tree = build_select_tree(&exprs);

        let mut match_arms = Vec::new();
        for i in 0..count {
            let block = &branches[i].block;
            let mut pattern;

            // Construct the corresponding Either path match
            if i < count - 1 {
                pattern = quote! { #path::Either::Left(val) };
                for _ in 0..i {
                    pattern = quote! { #path::Either::Right(#pattern) };
                }
            } else {
                pattern = quote! { val };
                for _ in 0..(count - 1) {
                    pattern = quote! { #path::Either::Right(#pattern) };
                }
            }

            // Place the user's block in the match branch,
            // and the value can be accessed by the block.
            match_arms.push(quote! { #pattern => { let _ = val; #block } });
            // To prevent the error message "val not used"
        }

        quote! { match #tree.await { #(#match_arms,)* } }
    }

    fn build_select_tree(exprs: &[Expr]) -> TokenStream {
        let count = exprs.len();
        let head = &exprs[0];

        let path = get_path();

        if count == 2 {
            let tail = &exprs[1];
            return quote! { #path::Select::new(#head, #tail) };
        }

        let tail_tree = build_select_tree(&exprs[1..]);
        quote! { #path::Select::new(#head, #tail_tree) }
    }
}