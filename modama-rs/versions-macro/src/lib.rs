use proc_macro::TokenStream;
use quote::quote;
use syn::{
    LitStr, Path, Token,
    parse::{Parse, ParseStream},
};
use winnow::{
    Parser,
    ascii::dec_uint,
    combinator::{alt, cut_err, eof, fail, repeat},
    error::{StrContext, StrContextValue},
};

#[proc_macro]
pub fn versions(tokens: TokenStream) -> TokenStream {
    match versions_impl(tokens.into()) {
        Ok(res) => res.into(),
        Err(e) => e.to_compile_error().into(),
    }
}

fn versions_impl(tokens: proc_macro2::TokenStream) -> Result<proc_macro2::TokenStream, syn::Error> {
    let MacroInput {
        crate_path,
        lit_str,
    } = syn::parse2::<MacroInput>(tokens.clone())?;
    let lit_str_value = lit_str.value();

    let input = lit_str_value.as_str();

    let result = parse_version_range.parse(input);

    let parsed = match result {
        Ok(v) => v,
        Err(e) => {
            // Extract the expected context from the error - use the most specific (last) context
            let substr = &lit_str_value[e.char_span().start..];
            let expected = if let Some(ctx) = e.inner().context().next() {
                match ctx {
                    StrContext::Label(label) => format!("expected '{}'", label),
                    StrContext::Expected(val) => match val {
                        StrContextValue::CharLiteral(c) => format!("expected '{}'", c),
                        StrContextValue::StringLiteral(s) => format!("expected '{}'", s),
                        StrContextValue::Description(d) => format!("expected {}", d),
                        _ => "invalid format".to_string(),
                    },
                    _ => "invalid version range".to_string(),
                }
            } else {
                "invalid version range".to_string()
            };

            // FIXME: once subspan support works on stable, use this for more precise error message
            return Err(syn::Error::new_spanned(
                &tokens,
                format!(
                    "Invalid version range '{}': {} (at offset {}, '{}')",
                    lit_str_value,
                    expected,
                    e.char_span().start,
                    substr
                ),
            ));
        }
    };

    if let ParsedVersionRange::Range(start, end) = parsed
        && let Err(error) = version_range::validate_numeric_bounds(start, end)
    {
        return Err(syn::Error::new_spanned(
            &tokens,
            format!("Invalid version range '{lit_str_value}': {error}"),
        ));
    }

    let version_range = crate_path.as_ref().map_or_else(
        || quote!(::version_range::VersionRange),
        |path| quote!(#path::VersionRange),
    );

    Ok(match parsed {
        ParsedVersionRange::Exact((major, minor, patch)) => {
            quote! { #version_range::__exact(#major, #minor, #patch) }
        }
        ParsedVersionRange::Range(None, None) => range_expression(&version_range, None, None),
        ParsedVersionRange::Range(None, Some(v2)) => {
            let v2_0 = v2.0;
            let v2_1 = v2.1;
            let v2_2 = v2.2;
            range_expression(&version_range, None, Some(quote!((#v2_0, #v2_1, #v2_2))))
        }
        ParsedVersionRange::Range(Some(v1), None) => {
            let v1_0 = v1.0;
            let v1_1 = v1.1;
            let v1_2 = v1.2;
            range_expression(&version_range, Some(quote!((#v1_0, #v1_1, #v1_2))), None)
        }
        ParsedVersionRange::Range(Some(v1), Some(v2)) => {
            let v1_0 = v1.0;
            let v1_1 = v1.1;
            let v1_2 = v1.2;
            let v2_0 = v2.0;
            let v2_1 = v2.1;
            let v2_2 = v2.2;
            range_expression(
                &version_range,
                Some(quote!((#v1_0, #v1_1, #v1_2))),
                Some(quote!((#v2_0, #v2_1, #v2_2))),
            )
        }
    })
}

fn range_expression(
    version_range: &proc_macro2::TokenStream,
    start: Option<proc_macro2::TokenStream>,
    end: Option<proc_macro2::TokenStream>,
) -> proc_macro2::TokenStream {
    let start = start.map_or_else(|| quote!(None), |value| quote!(Some(#value)));
    let end = end.map_or_else(|| quote!(None), |value| quote!(Some(#value)));
    quote! {
        match #version_range::__range(#start, #end) {
            Ok(range) => range,
            Err(error) => unreachable!("versions! validated an invalid range: {}", error),
        }
    }
}

struct MacroInput {
    crate_path: Option<Path>,
    lit_str: LitStr,
}

impl Parse for MacroInput {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        if input.peek(LitStr) {
            return Ok(Self {
                crate_path: None,
                lit_str: input.parse()?,
            });
        }

        let crate_path = input.parse()?;
        input.parse::<Token![,]>()?;
        let lit_str = input.parse()?;
        Ok(Self {
            crate_path: Some(crate_path),
            lit_str,
        })
    }
}

fn parse_dotdot(input: &mut &str) -> winnow::ModalResult<()> {
    repeat(2, '.')
        .context(StrContext::Expected(StrContextValue::Description("'..'")))
        .parse_next(input)
}

fn parse_triplet(input: &mut &str) -> winnow::ModalResult<(u64, u64, u64)> {
    let (maj, _, min, _, fix) = (
        dec_uint::<_, u64, _>,
        ".",
        dec_uint::<_, u64, _>,
        ".",
        dec_uint::<_, u64, _>,
    )
        .context(StrContext::Label("X.Y.Z"))
        .parse_next(input)?;

    Ok((maj, min, fix))
}

#[derive(Clone, Copy)]
enum ParsedVersionRange {
    Exact((u64, u64, u64)),
    Range(Option<(u64, u64, u64)>, Option<(u64, u64, u64)>),
}

fn parse_version_range(input: &mut &str) -> winnow::ModalResult<ParsedVersionRange> {
    alt((
        (parse_triplet, eof).map(|result| ParsedVersionRange::Exact(result.0)),
        parse_range,
    ))
    .parse_next(input)
}

fn parse_range(input: &mut &str) -> winnow::ModalResult<ParsedVersionRange> {
    (
        // one of...
        cut_err(alt((
            // X.Y.Z..
            (parse_triplet, cut_err(parse_dotdot)).map(|t| Some(t.0)),
            // ..
            (parse_dotdot).map(|_| None),
            fail.context(StrContext::Expected(StrContextValue::Description(
                "X.Y.Z or '..'",
            ))),
        ))),
        // one of...
        alt((
            // X.Y.Z<EOF>
            (parse_triplet.map(Some), eof).map(|t| t.0),
            // <EOF>
            eof.map(|_| None),
            fail.context(StrContext::Expected(StrContextValue::Description(
                "X.Y.Z or end of input",
            ))),
        )),
    )
        .map(|(start, end)| ParsedVersionRange::Range(start, end))
        .parse_next(input)
}

#[cfg(test)]
mod tests {
    use quote::quote;

    use crate::versions_impl;

    #[test]
    fn test_version_range_easy() {
        let result: proc_macro2::TokenStream = versions_impl(quote! { "1.0.0.." }).unwrap();
        println!("{:?}", result);
        let result: proc_macro2::TokenStream = versions_impl(quote! { "10.0.0.." }).unwrap();
        println!("{:?}", result);
        let result: proc_macro2::TokenStream = versions_impl(quote! { ".." }).unwrap();
        println!("{:?}", result);
    }
}
