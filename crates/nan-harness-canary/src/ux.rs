mod errors;
mod html;
mod output;
mod scenarios;
mod selection;
mod terminal;

use crate::app::UxArgs;
pub(crate) use errors::UxError;
use html::write_html;
use scenarios::load_scenarios;
use selection::select_scenarios;
use terminal::terminal_output;

pub(crate) fn run(arguments: &UxArgs) -> Result<(), UxError> {
    let scenarios = load_scenarios()?;
    if arguments.list {
        for scenario in &scenarios {
            println!("{}", scenario.id);
        }
        return Ok(());
    }

    let selected = select_scenarios(&scenarios, arguments.scenario.as_deref())?;
    for (index, scenario) in selected.iter().enumerate() {
        if index > 0 {
            println!();
        }
        println!("== {} ==", scenario.title);
        println!("Command: {}", scenario.command);
        println!("Appears when: {}", scenario.appears_when);
        println!("Result: {}", scenario.result);
        println!();
        println!("{}", terminal_output(scenario));
    }

    if let Some(path) = &arguments.html {
        write_html(path, &scenarios)?;
        eprintln!("UX catalog written to '{}'.", path.display());
    }
    Ok(())
}
