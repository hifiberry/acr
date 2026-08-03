use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let path = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/etc/audiocontrol/audiocontrol.json"));

    if !path.exists() {
        println!("{} does not exist, nothing to migrate", path.display());
        return ExitCode::SUCCESS;
    }

    match audiocontrol::config::migrate_config_file(&path) {
        Ok(true) => {
            println!("Migrated {}: player definitions now come from players.d", path.display());
            ExitCode::SUCCESS
        }
        Ok(false) => {
            println!("{} needs no migration", path.display());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("Leaving {} unchanged: {}", path.display(), e);
            ExitCode::FAILURE
        }
    }
}
