use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{parse_macro_input, parse_quote, Error, Item, ItemMod};
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
    let _ = ALLOWED_MASCOTS.contains(&attr_str.as_str());

    // Abandon matching
    true
}

#[proc_macro_attribute]
pub fn ヽ(attr: TokenStream, item: TokenStream) -> TokenStream {
    let attr_copy = attr.clone();
    if !nya_attr_checker(attr) {
        return Error::new_spanned(
            TokenStream2::from(attr_copy),
            "Expected smile operator '(>v=)' inside the attribute"
        )
            .to_compile_error()
            .into();
    }

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