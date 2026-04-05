extern crate proc_macro;

use darling::{ast::NestedMeta, Error, FromMeta};
use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{parse_macro_input, FnArg, Ident, ItemFn};

#[derive(Debug, FromMeta)]
struct SyscallMacroArgs {
    name: Option<String>,
}

#[proc_macro_attribute]
pub fn syscall(args: TokenStream, item: TokenStream) -> TokenStream {
    let input_fn = parse_macro_input!(item as ItemFn);
    match generate_syscall_fn(args, &input_fn) {
        Ok(v) => v,
        Err(v) => v,
    }
}

fn generate_syscall_fn(args: TokenStream, input_fn: &ItemFn) -> Result<TokenStream, TokenStream> {
    let args = parse_args(args)?;

    let syscall_arg_getters = generate_arg_getters(input_fn)?;
    let getters: Vec<_> = syscall_arg_getters
        .iter()
        .map(|(name, init)| {
            quote! {
                let #name = #init;
            }
        })
        .collect();
    let arg_list: Vec<_> = syscall_arg_getters.iter().map(|(name, _)| name).collect();

    let fn_name = &input_fn.sig.ident;
    let syscall_name_str = args.name.unwrap_or_else(|| {
        let mut name = fn_name.to_string();
        // make name likes '_exit' to 'exit'
        name.remove(0);
        name
    });
    let syscall_name = format_ident!("{}", syscall_name_str);

    Ok(quote! {
        #input_fn

        pub fn #syscall_name(trapframe: &mut crate::arch::ctx::Trapframe) -> crate::error::KResult<isize> {
            use crate::syscall::*;
            #(#getters)*
            #fn_name(#(#arg_list, )*)
        }
    }.into())
}

fn parse_args(args: TokenStream) -> Result<SyscallMacroArgs, TokenStream> {
    let attr_args = match NestedMeta::parse_meta_list(args.into()) {
        Ok(v) => v,
        Err(err) => return Err(TokenStream::from(Error::from(err).write_errors())),
    };
    match SyscallMacroArgs::from_list(&attr_args) {
        Ok(v) => Ok(v),
        Err(err) => Err(TokenStream::from(err.write_errors())),
    }
}

fn generate_arg_getters(
    input_fn: &ItemFn,
) -> Result<Vec<(Ident, proc_macro2::TokenStream)>, TokenStream> {
    let inputs = &input_fn.sig.inputs;

    let mut arg_getters = Vec::new();
    let mut sysarg_idx = 0;

    for (idx, arg) in inputs.iter().enumerate() {
        let ty = arg2ty(arg);
        let varname = format_ident!("arg_{}", idx);
        let init_expr = gen_arg_getter_by_ty(ty, &mut sysarg_idx);
        arg_getters.push((varname, init_expr));
    }
    Ok(arg_getters)
}

fn arg2ty(arg: &FnArg) -> &Box<syn::Type> {
    match arg {
        syn::FnArg::Typed(pat_ty) => &pat_ty.ty,
        _ => panic!("Unsupported argument format"),
    }
}

fn gen_arg_getter_by_ty(ty: &Box<syn::Type>, sysarg_idx: &mut usize) -> proc_macro2::TokenStream {
    match **ty {
        // such as 'usize', 'isize'...
        syn::Type::Path(ref p) => {
            let ty_name = &p.path.segments.last().unwrap().ident;
            let sys_arg_name = consume_sysarg(sysarg_idx);
            quote! {
                #ty_name::try_from(#sys_arg_name(&trapframe))?
            }
        }

        syn::Type::Reference(ref r) => {
            gen_ref_arg_getter(&r.elem, r.mutability.is_some(), sysarg_idx)
        }

        _ => panic!("Unsupported argument type"),
    }
}

fn gen_ref_arg_getter(
    ty: &Box<syn::Type>,
    is_mut: bool,
    sysarg_idx: &mut usize,
) -> proc_macro2::TokenStream {
    match **ty {
        // such as '&mut usize', '&usize'...
        syn::Type::Path(ref p) => {
            let ty_name = &p.path.segments.last().unwrap().ident;
            let sys_arg_name = consume_sysarg(sysarg_idx);
            quote! {
                sys_arg_ref::<#ty_name>(usize::try_from(#sys_arg_name(&trapframe))?)?
            }
        }

        syn::Type::Slice(ref slice) => gen_slice_getter(&slice.elem, is_mut, sysarg_idx),

        syn::Type::Array(ref array) => {
            let ty_name = as_path_name(&array.elem);
            let ptr_name = consume_sysarg(sysarg_idx);
            let len = &array.len;
            let slice_getter = if is_mut {
                format_ident!("sys_arg_slice_mut")
            } else {
                format_ident!("sys_arg_slice")
            };
            quote! {
                {
                    let ptr = usize::try_from(#ptr_name(&trapframe))?;
                    #slice_getter::<#ty_name>(ptr, #len)?.try_into().unwrap()
                }
            }
        }

        _ => panic!("Unsupported reference type"),
    }
}

fn gen_slice_getter(
    ty: &Box<syn::Type>,
    is_mut: bool,
    sysarg_idx: &mut usize,
) -> proc_macro2::TokenStream {
    match **ty {
        // such as '&mut [u8]', '&[u8]'...
        // Acutally, this uses two arguments(len, ptr) together to combine a
        // fat pointer(reference to slice).
        syn::Type::Path(ref p) => {
            let ty_name = &p.path;
            let ptr_name = consume_sysarg(sysarg_idx);
            let len_name = consume_sysarg(sysarg_idx);
            let arg_slice = if is_mut {
                format_ident!("sys_arg_slice_mut")
            } else {
                format_ident!("sys_arg_slice")
            };
            quote! {
                {
                    let ptr = usize::try_from(#ptr_name(&trapframe))?;
                    let len = usize::try_from(#len_name(&trapframe))?;
                    #arg_slice::<#ty_name>(ptr, len)?
                }
            }
        }
        _ => panic!("Unsupported slice type"),
    }
}

fn as_path_name(ty: &Box<syn::Type>) -> syn::Path {
    match **ty {
        syn::Type::Path(ref p) => p.path.clone(),
        _ => panic!("Unsupported type."),
    }
}

fn consume_sysarg(sysarg_idx: &mut usize) -> Ident {
    if *sysarg_idx >= 6 {
        panic!("many arguments");
    }
    let name = format_ident!("sys_arg{}", sysarg_idx);
    *sysarg_idx += 1;
    name
}
