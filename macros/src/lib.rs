use proc_macro::TokenStream;
use quote::{format_ident, quote, ToTokens};
use syn::{parse_macro_input, parse_quote, Error, FnArg, Item, ItemFn, ItemMod, Pat, Token};
use syn::parse::{Parse, ParseStream};
use syn::spanned::Spanned;

fn nya_attr_checker(attr: TokenStream) -> bool {
    // Lexical matching
    let mut iter = attr.clone().into_iter();
    match iter.next() {
        Some(proc_macro::TokenTree::Punct(p)) if p.as_char() == '^' => {}
        _ => return true,
    }
    match iter.next() {
        Some(proc_macro::TokenTree::Ident(i)) if i.to_string() == "v" => {}
        _ => return true,
    }
    match iter.next() {
        Some(proc_macro::TokenTree::Punct(p)) if p.as_char() == '^' => {}
        _ => return true,
    }
    let _ = iter.next().is_none();

    // Thesaurus matching
    let attr_str = attr.to_string().replace(" ", "");
    const ALLOWED_MASCOTS: &[&str] = &[
        "^v^",
        "^^v",
        ">v<",
        ">v<ノ",
        "^_^",
        "^v^ノ",
    ];
    ALLOWED_MASCOTS.contains(&attr_str.as_str())
}

#[proc_macro_attribute]
pub fn ヽ(attr: TokenStream, item: TokenStream) -> TokenStream {
    // let attr_copy = attr.clone();
    // if !nya_attr_checker(attr) {
    //     return Error::new_spanned(
    //         TokenStream2::from(attr_copy),
    //         "Expected smile operator '(>v=)' inside the attribute"
    //     )
    //         .to_compile_error()
    //         .into();
    // }

    match attr.to_string().as_str() {
        "'ε'" => init(item),
        _ => init(item)
    }
}
fn init(item: TokenStream) -> TokenStream {
    // 2. 解析 mod MyApp { ... }
    let mut input_mod = parse_macro_input!(item as ItemMod);

    // 检查是否有大括号内容 (mod xxx; 是不被允许的)
    let (_, items) = match &input_mod.content {
        Some(content) => content,
        None => return Error::new_spanned(&input_mod, "必须使用 'mod xxx { ... }' 格式").to_compile_error().into(),
    };

    // 3. 定义允许的函数名单及其属性
    // 格式：(函数名, 是否应该是 async)
    let allowed_functions = [
        ("master_setup", true),
        ("agent_setup", true),
        ("agent_main", true),
        ("agent_idle", false),
        ("on_panic", false),
        ("on_error", false),
        ("on_exit", false),
    ];

    // 4. 遍历模块内容进行“体检”
    for item in items {
        match item {
            Item::Fn(f) => {
                let fn_name = f.sig.ident.to_string();
                let fn_span = f.sig.ident.span();

                // 检查函数名是否在白名单中
                let (is_allowed, should_be_async) = match allowed_functions.iter().find(|(name, _)| *name == fn_name) {
                    Some(found) => (true, found.1),
                    None => (false, false),
                };

                if !is_allowed {
                    return Error::new(fn_span, format!("非法函数 '{}'：#[v] 模块内只允许定义特定的框架函数", fn_name))
                        .to_compile_error().into();
                }

                // 检查 async 匹配情况
                let is_async = f.sig.asyncness.is_some();
                if is_async != should_be_async {
                    let msg = if should_be_async { "必须是 async fn" } else { "不能是 async fn" };
                    return Error::new(f.sig.span(), format!("函数 '{}' {}", fn_name, msg))
                        .to_compile_error().into();
                }

                // 检查参数数量 (假设这些函数都不应该带参数)
                if !f.sig.inputs.is_empty() {
                    return Error::new(f.sig.inputs.span(), format!("函数 '{}' 不应该携带参数", fn_name))
                        .to_compile_error().into();
                }
            }
            Item::Use(_) => {}, // 允许使用 use 语句
            _ => {
                // 不允许定义 结构体、常量、宏等其他内容
                return Error::new_spanned(item, "#[v] 模块内只能包含指定的函数和 use 语句")
                    .to_compile_error().into();
            }
        }
    }

    let entry_fn: Item = parse_quote! {
        #[::uefi::entry] // 直接利用 uefi 库自带的 entry 宏
        fn main() -> ::uefi::Status {

            ::uefi::println!("Hello, World!");

            ::uefi::boot::stall(::core::time::Duration::from_secs(120));

            ::uefi::Status::SUCCESS
        }
    };

    // 3. 注入代码到模块内部
    // input_mod.content 是 Option<(Brace, Vec<Item>)>
    if let Some((_, items)) = &mut input_mod.content {
        items.push(entry_fn);
    } else {
        // 如果用户写的是 mod MyApp; (没有大括号)，则报错
        return Error::new_spanned(input_mod, "模块必须带有大括号内容")
            .to_compile_error()
            .into();
    }

    // 4. 直接输出修改后的模块
    let expanded = quote! {

        #[doc(hidden)]
        pub static __ONLY_ONE_V_MACRO_ALLOWED__: () = ();
        #input_mod
    };

    expanded.into()
}

struct TaskArgs {
    pool_size: usize,
}

impl Parse for TaskArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut pool_size = 1; // 默认大小
        if !input.is_empty() {
            let id: syn::Ident = input.parse()?;
            if id != "pool_size" {
                return Err(syn::Error::new(id.span(), "expected `pool_size`"));
            }
            input.parse::<Token![=]>()?;
            let lit: syn::LitInt = input.parse()?;
            pool_size = lit.base10_parse()?;
        }
        Ok(TaskArgs { pool_size })
    }
}

#[proc_macro_attribute]
pub fn task(attr: TokenStream, item: TokenStream) -> TokenStream {
    // 1. 解析 pool_size 参数（假设 TaskArgs 已定义）
    let args = parse_macro_input!(attr as TaskArgs);
    let pool_size = args.pool_size;

    // 2. 解析函数
    let mut f = parse_macro_input!(item as ItemFn);

    // 必须是 async
    if f.sig.asyncness.is_none() {
        return syn::Error::new_spanned(&f.sig, "task functions must be async")
            .to_compile_error()
            .into();
    }

    // 3. 提取标识符
    let task_ident = f.sig.ident.clone(); // 外部调用名
    let task_inner_ident = format_ident!("__{}_task", task_ident); // 内部异步状态机名

    // 4. 处理参数（同步包装器接收参数，闭包移动参数）
    let mut fargs = Vec::new();
    let mut full_args = Vec::new();
    for arg in &f.sig.inputs {
        if let FnArg::Typed(pat_type) = arg {
            fargs.push(arg.clone());
            if let Pat::Ident(pat_id) = &*pat_type.pat {
                full_args.push(pat_id.ident.clone());
            }
        }
    }

    // 5. 复制原函数并修改为私有内部函数
    let mut task_inner = f.clone();
    task_inner.sig.ident = task_inner_ident.clone();
    task_inner.vis = syn::Visibility::Inherited;

    // 6. 定义路径
    let executor_path = quote!(::uefi_async);
    let visibility = &f.vis;

    // 7. 生成代码
    let result = quote! {
        // A. 原始异步逻辑，改名后隐藏
        #[doc(hidden)]
        #task_inner

        // B. 用户调用的入口函数
        #visibility fn #task_ident(#(#fargs),*) -> Result<#executor_path::SpawnToken, #executor_path::SpawnError> {
            const POOL_SIZE: usize = #pool_size;

            // 声明静态内存布局（预留空间）
            // 利用 task_pool_size 计算该异步函数对应的 Pool 所需字节数
            static POOL_STORAGE: #executor_path::TaskPoolLayout<
                {#executor_path::task_pool_size::<_, _, _, POOL_SIZE>(#task_inner_ident)},
                {#executor_path::task_pool_align::<_, _, _, POOL_SIZE>(#task_inner_ident)},
            > = #executor_path::TaskPoolLayout([0u8; {#executor_path::task_pool_size::<_, _, _, POOL_SIZE>(#task_inner_ident)}]);

            // 内部类型恢复函数
            // 将字节数组强转为带具体 Future 类型的 TaskPool 引用
            fn __get_typed_pool<F, Args, Fut>(_: F) -> &'static #executor_path::TaskPool<Fut, POOL_SIZE>
            where
                F: #executor_path::TaskFn<Args, Fut = Fut>,
                Fut: ::core::future::Future<Output = ()> + 'static + Send + Sync,
            {
                unsafe {
                    &*(POOL_STORAGE.as_ptr() as *const #executor_path::TaskPool<Fut, POOL_SIZE>)
                }
            }

            // 获取池子引用
            let pool = __get_typed_pool(#task_inner_ident);

            // 执行分配逻辑
            // 构造闭包，移动参数，并启动任务
            // pool.spawn(move || #task_inner_ident(#(#full_args),*))
            todo!()
        }
    };

    result.into()
}