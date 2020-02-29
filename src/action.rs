use crate::gml::{compiler::{Compiler, mappings}, Context, runtime::{Instruction, Node, Runtime}, Value};
use gm8exe::asset::etc::CodeAction;

/// Consts which match those used in GM8
pub mod kind {
    pub const NORMAL: u32 = 0;
    pub const BEGIN_GROUP: u32 = 1;
    pub const END_GROUP: u32 = 2;
    pub const ELSE: u32 = 3;
    pub const EXIT: u32 = 4;
    pub const REPEAT: u32 = 5;
    pub const VARIABLE: u32 = 6;
    pub const CODE: u32 = 7;
}
pub mod execution_type {
    pub const NONE: u32 = 0;
    pub const FUNCTION: u32 = 1;
    pub const CODE: u32 = 2;
}

/// A drag-n-drop action.
#[derive(Debug)]
pub struct Action {
    /// The original index of this action in its list, starting at 0
    pub index: usize,

    /// The target ID. May be self (-1) or other (-2) or an object or instance id.
    /// A value of None means applies_to_something was false.
    pub target: Option<i32>,

    /// The arguments to be passed to the function or code body
    pub args: Box<[Node]>,

    /// Whether the "relative" checkbox was used. This is always passed to Context, but usually ignored.
    pub relative: bool,

    /// If this is a question action, this flag means the bool result will be inverted.
    pub invert_condition: bool,

    /// The body of this action to be executed
    pub body: Body,

    /// The 'if' and 'else' actions under this one, if this action is a question.
    pub if_else: Option<(Box<[Action]>, Box<[Action]>)>,
}

/// Abstraction for a tree of Actions
/// Note that Vec is necessary here due to functions such as object_event_add and object_event_clear
#[derive(Debug)]
pub struct Tree(Vec<Action>);

pub enum Body {
    Function(fn(&mut Runtime, &mut Context, &[Value]) -> Value),
    Code(Vec<Instruction>),
}

impl std::fmt::Debug for Body {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> Result<(), std::fmt::Error> {
        match self {
            Body::Function(_) => write!(f, "Body::Function(..)"),
            Body::Code(c) => write!(f, "Body::Code({:?})", c),
        }
    }
}

impl Tree {
    /// Turn a list of gm8exe CodeActions into an Action tree.
    pub fn from_list(list: &[CodeAction], compiler: &mut Compiler) -> Result<Self, String> {
        let mut iter = list.iter().enumerate().peekable();
        Ok(Self(Self::from_iter(&mut iter, compiler, false)?))
    }

    fn from_iter<'a, T>(iter: &mut std::iter::Peekable<T>, compiler: &mut Compiler, stop_at_end_action: bool) -> Result<Vec<Action>, String> where T: Iterator<Item=(usize, &'a CodeAction)> {
        let mut output = Vec::new();

        while let Some((i, action)) = iter.next() {
            let if_else = if action.is_condition {
                let if_body = Self::from_iter(iter, compiler, true)?;
                let else_body = if let Some((_, CodeAction {action_kind: kind::ELSE, ..})) = iter.peek() {
                    Self::from_iter(iter, compiler, true)?
                } else {
                    Vec::new()
                };
                Some((if_body.into_boxed_slice(), else_body.into_boxed_slice()))
            } else {
                None
            };

            match action.execution_type {
                execution_type::NONE => (),
                execution_type::FUNCTION => {
                    if let Some((_, f_ptr, _)) = mappings::FUNCTIONS.iter().find(|(n, _, _)| n == &action.fn_name) {
                        output.push(Action {
                            index: i,
                            target: if action.applies_to_something {Some(action.applies_to)} else {None},
                            args: action.param_strings.iter().map(|x| compiler.compile_expression(x)).collect::<Result<Vec<_>, _>>().map_err(|e| e.message)?.into_boxed_slice(),
                            relative: action.is_relative,
                            invert_condition: action.invert_condition,
                            body: Body::Function(*f_ptr),
                            if_else,
                        });
                    } else {
                        return Err(format!("Unknown function: {} in action {}", action.fn_name, i));
                    }
                },
                execution_type::CODE | _ => {
                    if action.action_kind == kind::CODE {
                        output.push(Action {
                            index: i,
                            target: if action.applies_to_something {Some(action.applies_to)} else {None},
                            args: Box::new([]),
                            relative: action.is_relative,
                            invert_condition: action.invert_condition,
                            body: match action.param_strings.get(0) {
                                Some(code) => Body::Code(compiler.compile(code).map_err(|e| e.message)?),
                                None => Body::Code(Vec::new()),
                            },
                            if_else,
                        });
                    }
                    else {
                        output.push(Action {
                            index: i,
                            target: if action.applies_to_something {Some(action.applies_to)} else {None},
                            args: action.param_strings.iter().map(|x| compiler.compile_expression(x)).collect::<Result<Vec<_>, _>>().map_err(|e| e.message)?.into_boxed_slice(),
                            relative: action.is_relative,
                            invert_condition: action.invert_condition,
                            body: Body::Code(compiler.compile(&action.fn_code).map_err(|e| e.message)?),
                            if_else,
                        });
                    }
                },
            }

            if action.action_kind == kind::END_GROUP && stop_at_end_action {
                break;
            }
        }

        Ok(output)
    }
}
