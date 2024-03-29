use std::assert_matches::assert_matches;

use anyhow::Context;
use itertools::Itertools;
use wit_parser::{
    FunctionKind, InterfaceId, Package, Resolve, Type, TypeDefKind, TypeId, TypeOwner,
};

const INDENTATION: &str = "    ";

pub trait ToWitSyntax {
    fn to_wit_syntax(&self, resolve: &Resolve) -> anyhow::Result<String>;
}

impl ToWitSyntax for Resolve {
    fn to_wit_syntax(&self, resolve: &Resolve) -> anyhow::Result<String> {
        let mut output = OutputBuilder::new();
        for (_, package_def) in &resolve.packages {
            add_package(resolve, &mut output, package_def)?;
        }

        Ok(output.to_string())
    }
}

fn add_package(
    resolve: &Resolve,
    output: &mut OutputBuilder,
    package_def: &Package,
) -> anyhow::Result<()> {
    output.add_lines(&format!("package {};", package_def.name));
    output.add_empty_line();
    for (_, interface_id) in &package_def.interfaces {
        add_interface(resolve, output, *interface_id)?;
    }
    Ok(())
}

fn add_interface(
    resolve: &Resolve,
    output: &mut OutputBuilder,
    interface_id: InterfaceId,
) -> anyhow::Result<()> {
    let interface_def = resolve.interfaces.get(interface_id).unwrap();
    output.add_lines(&format!(
        "interface {} {{",
        interface_def
            .name
            .as_ref()
            .map(|n| n.as_str())
            .unwrap_or_default()
    ));
    output.indent();
    for (_, type_id) in &interface_def.types {
        let type_def = resolve.types.get(*type_id).unwrap();
        match &type_def.name {
            None => {
                asset_type_def_kind_inline(&type_def.kind);
                // unnamed/inlined types don't need to be in output.
            }
            Some(name) => {
                asset_type_def_kind_named(&type_def.kind);
                let val = type_def_kind_standalone(
                    *type_id,
                    &type_def.kind,
                    name,
                    resolve,
                    &type_def.owner,
                )?;
                output.add_lines(&val);
            }
        }
    }
    output.outdent();
    output.add_lines(&format!("}}"));
    Ok(())
}

// this function is also from `translations`, where not all types are known yet. So make `pub` and allow `TypeDefKind::Unknown` in `inline_type_rep`.
// might make sense to have a separate function in translations, not sure.
pub fn inline_type_name(type_: &Type, resolve: &Resolve) -> anyhow::Result<String> {
    Ok(String::from(match type_ {
        Type::Bool => "bool",
        Type::U8 => "u8",
        Type::U16 => "u16",
        Type::U32 => "u32",
        Type::U64 => "u64",
        Type::S8 => "s8",
        Type::S16 => "s16",
        Type::S32 => "s32",
        Type::S64 => "s64",
        Type::Float32 => "float32",
        Type::Float64 => "float64",
        Type::Char => "char",
        Type::String => "string",
        Type::Id(type_id) => {
            return Ok(inline_type_rep(type_id, resolve)?);
        }
    }))
}

fn inline_type_rep(type_id: &TypeId, resolve: &Resolve) -> anyhow::Result<String> {
    let type_def = resolve.types.get(*type_id).context("Can't find type.")?;
    Ok(match &type_def.name {
        Some(name) => {
            if !matches!(type_def.kind, TypeDefKind::Unknown) {
                asset_type_def_kind_named(&type_def.kind);
            }
            name.clone()
        }
        None => {
            asset_type_def_kind_inline(&type_def.kind);
            type_def_kind_inline(&type_def.kind, resolve)?
        }
    })
}
fn type_def_kind_inline(type_def_kind: &TypeDefKind, resolve: &Resolve) -> anyhow::Result<String> {
    asset_type_def_kind_inline(type_def_kind);
    Ok(String::from(match type_def_kind {
        TypeDefKind::List(type_) => {
            let type_name = inline_type_name(type_, resolve)?;
            format!("list<{type_name}>")
        }
        TypeDefKind::Tuple(_) => todo!(),
        TypeDefKind::Option(type_) => {
            let type_name = inline_type_name(type_, resolve)?;
            format!("option<{type_name}>")
        }
        TypeDefKind::Result(_) => todo!(),
        _ => unreachable!(),
    }))
}
fn type_def_kind_standalone(
    type_id: TypeId,
    type_def_kind: &TypeDefKind,
    name: &String,
    resolve: &Resolve,
    type_owner: &TypeOwner,
) -> anyhow::Result<String> {
    asset_type_def_kind_named(type_def_kind);

    Ok(String::from(match type_def_kind {
        TypeDefKind::Record(record) => {
            let mut output = format!("record {} {{\n", name);
            for case in &record.fields {
                let type_name = inline_type_name(&case.ty, resolve)?;
                output += &format!("{}{}: {},\n", INDENTATION, case.name, type_name);
            }
            output += &format!("}}\n");
            output
        }
        TypeDefKind::Resource => {
            let mut output = format!("resource {} {{\n", &name);

            let interface_id = match type_owner {
                TypeOwner::Interface(interface_id) => *interface_id,
                TypeOwner::World(_) => todo!(),
                TypeOwner::None => todo!(),
            };
            let interface = resolve.interfaces.get(interface_id).unwrap();

            for (func_name, function) in &interface.functions {
                let func_name = clean_func_name(&func_name);
                if !matches!(
                    function.kind,
                    FunctionKind::Constructor(id) | FunctionKind::Static(id) | FunctionKind::Method(id) if id == type_id
                ) {
                    continue;
                }

                let return_ = match &function.results {
                    wit_parser::Results::Named(returns) => {
                        if returns.is_empty() {
                            None
                        } else {
                            todo!()
                        }
                    }
                    wit_parser::Results::Anon(return_type) => {
                        Some(inline_type_name(&return_type, resolve)?)
                    }
                };
                let return_ = match return_ {
                    Some(return_) => format!(" -> {return_}"),
                    None => String::new(),
                };
                let params = function
                    .params
                    .iter()
                    .map(|(param_name, param_type)| {
                        let param_type_name = inline_type_name(&param_type, resolve).unwrap();

                        format!("{}: {}", param_name, param_type_name)
                    })
                    .collect_vec()
                    .join(", ");
                let declaration = match function.kind {
                    FunctionKind::Freestanding => unreachable!(),
                    FunctionKind::Method(_) => {
                        format!("{INDENTATION}{func_name}: func({params}){return_};\n")
                    }
                    FunctionKind::Static(_) => {
                        format!("{INDENTATION}{func_name}: static func({params}){return_};\n")
                    }
                    FunctionKind::Constructor(_) => {
                        format!("{INDENTATION}constructor({params});\n")
                    }
                };
                output += &declaration;
            }

            output += &format!("}}\n");
            output
        }
        TypeDefKind::Flags(_) => todo!(),
        TypeDefKind::Variant(variant) => {
            let mut output = format!("variant {name} {{\n");
            for case in &variant.cases {
                match case.ty {
                    Some(type_) => {
                        let type_name = inline_type_name(&type_, resolve).unwrap();
                        output += &format!("{}{}({}),\n", INDENTATION, case.name, type_name);
                    }
                    None => {
                        output += &format!("{}{},\n", INDENTATION, case.name);
                    }
                }
            }
            output += &format!("}}\n");
            output
        }
        TypeDefKind::Type(type_) => {
            let type_name = inline_type_name(&type_, resolve).unwrap();
            format!("type {name} = {type_name};\n")
        }
        TypeDefKind::Enum(e) => {
            let mut output = format!("enum {} {{\n", name);
            for case in &e.cases {
                output += &format!("{}{},\n", INDENTATION, case.name);
            }
            output += &format!("}}\n");
            output
        }
        _ => unreachable!(),
    }))
}

fn asset_type_def_kind_inline(type_def_kind: &TypeDefKind) {
    assert_matches!(
        type_def_kind,
        TypeDefKind::Tuple(_)
            | TypeDefKind::Option(_)
            | TypeDefKind::Result(_)
            | TypeDefKind::List(_)
    )
}
fn asset_type_def_kind_named(type_def_kind: &TypeDefKind) {
    assert_matches!(
        type_def_kind,
        TypeDefKind::Record(_)
            | TypeDefKind::Flags(_)
            | TypeDefKind::Variant(_)
            | TypeDefKind::Enum(_)
            | TypeDefKind::Type(_)
            | TypeDefKind::Resource
    )
}

fn clean_func_name(name: &str) -> &str {
    const CONSTRUCTOR: &str = "[constructor]";
    if name.starts_with(CONSTRUCTOR) {
        return &name[CONSTRUCTOR.len()..];
    }
    let dot_pos = name.chars().position(|c| c == '.').unwrap();
    &name[dot_pos + 1..]
}

#[derive(Default)]
struct OutputBuilder {
    val: String,
    indentation: usize,
}
impl ToString for OutputBuilder {
    fn to_string(&self) -> String {
        self.val.to_owned()
    }
}
impl OutputBuilder {
    pub fn new() -> Self {
        Default::default()
    }
    // pub fn add_line(&mut self, indentation: usize, s: &str) {
    //     let indentation = std::iter::repeat(INDENTATION).take(indentation).join("");
    //     self.val += &format!("{indentation}{s}\n");
    // }
    pub fn add_lines(&mut self, s: &str) {
        let indentation = std::iter::repeat(INDENTATION)
            .take(self.indentation)
            .join("");
        let s = s
            .lines()
            .map(|line| format!("{indentation}{line}\n"))
            .join("");
        self.val += &format!("{s}");
    }
    pub fn add_empty_line(&mut self) {
        self.val += "\n";
    }
    pub fn indent(&mut self) {
        self.indentation += 1;
    }
    pub fn outdent(&mut self) {
        self.indentation -= 1;
    }
}
