//! A workflow killed mid-step completes with each step executed exactly
//! once (`PLAN.md` Milestone 56's first "done when").
//!
//! The order workflow reserves stock, charges, and ships. Each step's
//! effect is a row in `effects`, written in the same transaction as the
//! step's journal entry. The binary runs the workflow and, when told to,
//! kills its own process: after a step, or in the middle of one before it
//! commits. When the binary runs again the workflow resumes, and the
//! effects table shows each step once.

use rustnative_durable::{Workflow, WorkflowContext, WorkflowError};
use serde::{Deserialize, Serialize};

/// Where to kill the process, from the `CRASH` environment variable:
/// `after:<step>` or `inside:<step>`.
#[must_use]
pub fn crash_point() -> Option<(String, String)> {
    let value = std::env::var("CRASH").ok()?;
    let (when, step) = value.split_once(':')?;
    Some((when.to_owned(), step.to_owned()))
}

fn maybe_crash(when: &str, step: &str) {
    if crash_point().is_some_and(|(at, name)| at == when && name == step) {
        // The process dies with no destructors and no flushing, as in a
        // power cut.
        std::process::abort();
    }
}

/// An order.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Order {
    /// Its id.
    pub id: String,
    /// The amount, in cents.
    pub amount: i64,
}

/// The order workflow.
pub struct Fulfil;

#[async_trait::async_trait]
impl Workflow for Fulfil {
    const KIND: &'static str = "fulfil";
    type Input = Order;
    type Output = String;

    async fn run(
        &self,
        context: &mut WorkflowContext,
        order: Order,
    ) -> Result<String, WorkflowError> {
        for step in ["reserve", "charge", "ship"] {
            let order_id = order.id.clone();
            let amount = order.amount;
            context.transactional_step(step, |transaction| {
                transaction
                    .execute(
                        "INSERT INTO effects (step, order_id, amount) VALUES (?1, ?2, ?3)",
                        rusqlite::params![step, order_id, amount],
                    )
                    .map_err(|error| error.to_string())?;
                maybe_crash("inside", step);
                Ok(())
            })?;
            maybe_crash("after", step);
        }
        Ok(format!("order {} shipped", order.id))
    }
}

/// Creates the application's table.
///
/// # Errors
///
/// SQLite refused.
pub fn schema(connection: &rusqlite::Connection) -> rusqlite::Result<()> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS effects (step TEXT NOT NULL, order_id TEXT NOT NULL, amount INTEGER NOT NULL);",
    )
}
