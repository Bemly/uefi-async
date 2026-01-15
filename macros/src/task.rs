use std::str::FromStr;

use darling::FromMeta;
use darling::export::NestedMeta;
use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote};
use syn::visit::{self, Visit};
use syn::{Expr, ExprLit, FnArg, ItemFn, Lit, LitInt, ReturnType, Type, Visibility};


pub(crate) fn task(attr: TokenStream, item: TokenStream) -> TokenStream {

    let f: ItemFn = match syn::parse2(item.clone()) {
        Ok(x) => x,
        Err(e) => return e.to_compile_error(),
    };

    let mut args = Vec::new();
    let mut fargs = f.sig.inputs.clone();

    for arg in fargs.iter_mut() {
        match arg {
            syn::FnArg::Typed(t) => {
                match t.pat.as_mut() {
                    syn::Pat::Ident(id) => {
                        id.mutability = None;
                        args.push((id.clone(), t.attrs.clone()));
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    let generics = &f.sig.generics;
    let where_clause = &f.sig.generics.where_clause;
    let unsafety = &f.sig.unsafety;
    let visibility = &f.vis;
    let task_outer_attrs = &f.attrs;

    // assemble the original input arguments,
    // including any attributes that may have
    // been applied previously
    let mut full_args = Vec::new();
    for (arg, cfgs) in args {
        full_args.push(quote!(
            #(#cfgs)*
            #arg
        ));
    }

    let task_ident = f.sig.ident.clone();
    let task_inner_ident = format_ident!("__{}_task", task_ident);
    let mut fargs = f.sig.inputs.clone();

    let mut task_inner_function = f.clone();
    let task_inner_function_ident = format_ident!("__{}_task_inner_function", task_ident);
    task_inner_function.sig.ident = task_inner_function_ident.clone();
    task_inner_function.vis = Visibility::Inherited;
    let task_inner_body = quote! {
        #task_inner_function
        // SAFETY: All the preconditions to `#task_ident` apply to
        //         all contexts `#task_inner_ident` is called in
        #unsafety {
            #task_inner_function_ident(#(#full_args,)*)
        }
    };

    let executor = quote!(::uefi_async);
    let pool_size = 1usize;

    let task_inner_future_output = match &f.sig.output {
        ReturnType::Default => quote! {-> impl ::core::future::Future<Output = ()>},
        // Special case the never type since we can't stuff it into a `impl Future<Output = !>`
        ReturnType::Type(arrow, maybe_never) if f.sig.asyncness.is_some() && matches!(**maybe_never, Type::Never(_)) => {
            quote! {
                #arrow impl ::core::future::Future<Output=#executor::Never>
            }
        },
        ReturnType::Type(arrow, maybe_never) if matches!(**maybe_never, Type::Never(_)) => quote! {
            #arrow #maybe_never
        },
        // Grab the arrow span, why not
        ReturnType::Type(arrow, typ) if f.sig.asyncness.is_some() => quote! {
            #arrow impl ::core::future::Future<Output = #typ>
        },
        // We assume that if `f` isn't async, it must return `-> impl Future<...>`
        // This is checked using traits later
        ReturnType::Type(arrow, typ) => quote! {
            #arrow #typ
        },
    };

    let task_inner = quote! {
        #visibility fn #task_inner_ident #generics (#fargs)
        #task_inner_future_output
        #where_clause
        {
            #task_inner_body
        }
    };

    let task_outer_body = quote! {
        // const fn __task_pool_get<F, Args, Fut>(_: F) -> &'static #executor::TaskPool<Fut, POOL_SIZE>
        // where
        //     F: #executor::TaskFn<Args, Fut = Fut>,
        //     Fut: ::core::future::Future<Output = ()> + 'static + ::core::marker::Send + ::core::marker::Sync,
        // {
        //     unsafe { &*POOL.get().cast() }
        // }

        const POOL_SIZE: usize = #pool_size;
        static POOL: #executor::TaskPoolLayout<
            {#executor::task_pool_size::<_, _, _, POOL_SIZE>(#task_inner_ident)},
            {#executor::task_pool_align::<_, _, _, POOL_SIZE>(#task_inner_ident)},
        > = unsafe { ::core::mem::transmute(#executor::task_pool_new::<_, _, _, POOL_SIZE>(#task_inner_ident)) };
        // unsafe { __task_pool_get(#task_inner_ident).spawn(move || #task_inner_ident()) }
    };

    quote! {
        // #[doc(hidden)]
        #task_inner

        #(#task_outer_attrs)*
        #visibility #unsafety fn #task_ident #generics (#fargs) #where_clause{
            #task_outer_body
        }
    }
}