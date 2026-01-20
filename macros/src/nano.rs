use std::iter::once;
use proc_macro2::{TokenStream, TokenTree};
use quote::{quote, format_ident};
use syn::{
    parse::{Parse, ParseStream},
    parse2,
    punctuated::Punctuated,
    Expr, Ident, Token, Result,
};

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
            });
        }
    }

    quote! {
        #(#declarations)*
    }
}