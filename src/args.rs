use std::collections::VecDeque;

use crate::errors::UsageError;

struct Opt {
    /// Argument
    name: &'static str,
    /// Alternative names for the option
    alt_names: Option<&'static Vec<&'static str>>,
    /// Whether the option should be displayed in the help
    display: bool,
    /// Description of the option
    description: &'static str,
    implemented: bool,
    fun: Box<dyn FnMut(&str, &mut VecDeque<String>) -> Result<(), UsageError>>,
}
