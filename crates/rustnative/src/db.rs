//! `rustnative db` (`PLAN.md` Milestone 49, `C38`): migrations generated
//! from the model, applied with a dry run, reversed, and squashed.
//!
//! The model is `schema.toml`; the history is `migrations/`. `db diff`
//! writes the next migration pair; when a column disappears while another
//! of its type appears, it asks whether that is a rename (or takes the
//! answer from `--rename table.from:to`), so data is not dropped by
//! accident.

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

use rustnative_server::db::migrate::{Migration, Migrations};
use rustnative_server::db::schema::{PossibleRename, Schema, diff, squash};

use crate::error::{Error, Result};
use crate::project::Project;

/// `rustnative db`.
#[derive(Debug, clap::Subcommand)]
pub enum DbCommand {
    /// Write the next migration from the model (`schema.toml`).
    Diff {
        /// The new migration's name (`add_email`).
        name: String,
        /// Confirmed renames, `table.from:to`; without them, a possible
        /// rename is asked about.
        #[arg(long = "rename")]
        renames: Vec<String>,
        /// Treat every possible rename as a drop and an add, without asking.
        #[arg(long)]
        no_prompt: bool,
    },
    /// Apply pending migrations to a database file.
    Migrate {
        /// The database file.
        database: PathBuf,
        /// List what would be applied, and apply nothing.
        #[arg(long)]
        dry_run: bool,
    },
    /// Reverse migrations down to `to` (or all of them).
    Rollback {
        /// The database file.
        database: PathBuf,
        /// The migration to stop at (it stays applied).
        #[arg(long)]
        to: Option<String>,
    },
    /// Fold every migration into one.
    Squash {
        /// The squashed migration's name.
        name: String,
    },
}

fn io(what: String) -> impl FnOnce(std::io::Error) -> Error {
    move |cause| Error::Io { what, cause }
}

fn usage(error: impl std::fmt::Display) -> Error {
    Error::Usage(error.to_string())
}

fn next_number(directory: &Path) -> u32 {
    let migrations = Migrations::from_dir(directory).unwrap_or_default();
    migrations
        .0
        .iter()
        .filter_map(|migration| migration.name.split('_').next()?.parse::<u32>().ok())
        .max()
        .unwrap_or(0)
        + 1
}

fn write_pair(directory: &Path, migration: &Migration) -> Result<()> {
    std::fs::create_dir_all(directory).map_err(io(format!("create {}", directory.display())))?;
    for (kind, sql) in [("up", &migration.up), ("down", &migration.down)] {
        let path = directory.join(format!("{}.{kind}.sql", migration.name));
        std::fs::write(&path, sql).map_err(io(format!("write {}", path.display())))?;
        println!("db: wrote {}", path.display());
    }
    Ok(())
}

fn parse_rename(text: &str) -> Result<PossibleRename> {
    let (table, columns) =
        text.split_once('.').ok_or_else(|| usage(format!("{text:?}: expected table.from:to")))?;
    let (from, to) = columns
        .split_once(':')
        .ok_or_else(|| usage(format!("{text:?}: expected table.from:to")))?;
    Ok(PossibleRename { table: table.into(), from: from.into(), to: to.into() })
}

/// Runs a `db` command in the project containing `here`.
///
/// # Errors
///
/// The project, model, migrations, or database cannot be read or written.
pub fn run(here: &Path, command: &DbCommand) -> Result<()> {
    let project = Project::find(here)?;
    let directory = project.root.join("migrations");
    match command {
        DbCommand::Diff { name, renames, no_prompt } => {
            let model_path = project.root.join("schema.toml");
            let model = std::fs::read_to_string(&model_path)
                .map_err(io(format!("read {}", model_path.display())))?;
            let desired = Schema::parse(&model).map_err(usage)?;
            let history = Migrations::from_dir(&directory).unwrap_or_default();
            let current = Schema::from_migrations(&history).map_err(usage)?;
            let mut confirmed =
                renames.iter().map(|rename| parse_rename(rename)).collect::<Result<Vec<_>>>()?;
            let plan = diff(&current, &desired, &confirmed);
            if !*no_prompt {
                let stdin = std::io::stdin();
                for possible in plan.possible_renames {
                    print!(
                        "db: did `{}.{}` become `{}`? A rename keeps its data; otherwise it is dropped. [y/N] ",
                        possible.table, possible.from, possible.to
                    );
                    let _ = std::io::stdout().flush();
                    let mut answer = String::new();
                    let _ = stdin.lock().read_line(&mut answer);
                    if answer.trim().eq_ignore_ascii_case("y") {
                        confirmed.push(possible);
                    }
                }
            }
            let plan = diff(&current, &desired, &confirmed);
            if plan.is_empty() {
                println!("db: the model and the migrations agree; nothing to write");
                return Ok(());
            }
            let migration = Migration {
                name: format!("{:04}_{name}", next_number(&directory)),
                up: plan.up,
                down: plan.down,
            };
            write_pair(&directory, &migration)
        }
        DbCommand::Migrate { database, dry_run } => {
            let migrations = Migrations::from_dir(&directory)
                .map_err(io(format!("read {}", directory.display())))?;
            let mut connection =
                rustnative_server::db::rusqlite::Connection::open(database).map_err(usage)?;
            if *dry_run {
                for migration in migrations.pending(&connection).map_err(usage)? {
                    println!("db: would apply {}\n{}", migration.name, migration.up);
                }
                return Ok(());
            }
            for name in migrations.apply(&mut connection).map_err(usage)? {
                println!("db: applied {name}");
            }
            Ok(())
        }
        DbCommand::Rollback { database, to } => {
            let migrations = Migrations::from_dir(&directory)
                .map_err(io(format!("read {}", directory.display())))?;
            let mut connection =
                rustnative_server::db::rusqlite::Connection::open(database).map_err(usage)?;
            for name in migrations.rollback(&mut connection, to.as_deref()).map_err(usage)? {
                println!("db: reversed {name}");
            }
            Ok(())
        }
        DbCommand::Squash { name } => {
            let migrations = Migrations::from_dir(&directory)
                .map_err(io(format!("read {}", directory.display())))?;
            let squashed = squash(&migrations, &format!("0001_{name}")).map_err(usage)?;
            let old = directory.with_file_name("migrations.before-squash");
            std::fs::rename(&directory, &old)
                .map_err(io(format!("move {}", directory.display())))?;
            println!("db: the previous history is kept in {}", old.display());
            write_pair(&directory, &squashed)
        }
    }
}
