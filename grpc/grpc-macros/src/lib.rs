#![doc = include_str!("../README.md")]

extern crate proc_macro;

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{quote, TokenStreamExt};
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::{parse_macro_input, Attribute, FnArg, Ident, Signature, Token};

#[derive(Debug)]
struct GrpcMethod {
    outer_attrs: Vec<Attribute>,
    inner_attrs: Vec<Attribute>,
    signature: Signature,
    _terminating_semi: Token![;],
}

impl Parse for GrpcMethod {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        Ok(GrpcMethod {
            outer_attrs: input.call(Attribute::parse_outer)?,
            inner_attrs: input.call(Attribute::parse_inner)?,
            signature: input.parse()?,
            _terminating_semi: input.parse()?,
        })
    }
}

impl GrpcMethod {
    fn instantiate_method(&self, tonic_method: GrpcMethodAttribute) -> TokenStream2 {
        let mut tokens = TokenStream2::new();

        tokens.append_all(&self.inner_attrs);
        tokens.append_all(&self.outer_attrs);

        let grpc_client_struct = tonic_method.client;
        let grpc_method_name = tonic_method.method;

        let signature = self.signature.clone();
        let params: Vec<_> = self
            .signature
            .inputs
            .iter()
            .filter_map(|arg| {
                let FnArg::Typed(arg) = arg else {
                    return None;
                };
                Some(&arg.pat)
            })
            .collect();

        let method = quote! {
            pub #signature {
                // 256 mb, future proof as celesita blocks grow
                const MAX_MSG_SIZE: usize = 256 * 1024 * 1024;

                let mut client = #grpc_client_struct :: new(
                    self.transport.clone(),
                )
                .max_decoding_message_size(MAX_MSG_SIZE)
                .max_encoding_message_size(MAX_MSG_SIZE);

                let param = crate::grpc::IntoGrpcParam::into_parameter(( #( #params ),* ));
                let request = ::tonic::Request::new(param);
                let response = client. #grpc_method_name (request).await;
                crate::grpc::FromGrpcResponse::try_from_response(response?.into_inner())
            }
        };

        tokens.extend(method);

        tokens
    }
}

#[derive(Debug)]
struct GrpcMethodAttribute {
    method: Ident,
    client: Punctuated<Ident, Token![::]>,
}

impl Parse for GrpcMethodAttribute {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut parsed = Punctuated::<Ident, Token![::]>::parse_separated_nonempty(input)?;

        let method = parsed.pop().expect("expected client method").into_value();
        parsed.pop_punct();
        let client = parsed;

        Ok(GrpcMethodAttribute { method, client })
    }
}

/// Annotate a function signature passing ServiceClient method to be called
#[proc_macro_attribute]
pub fn grpc_method(attr: TokenStream, item: TokenStream) -> TokenStream {
    let attributes = parse_macro_input!(attr as GrpcMethodAttribute);
    let method_sig = parse_macro_input!(item as GrpcMethod);

    let method = method_sig.instantiate_method(attributes);

    method.into()
}
