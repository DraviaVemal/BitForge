mod core;
mod utils;

use core::cli::AppArgs;
use utils::logging;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    logging::init(AppArgs::peek_log_level().as_deref());
    let args = AppArgs::parse();
    core::app::run(args).await
}
