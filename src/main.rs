mod core;
mod utils;

use core::cli::AppArgs;
use utils::logging;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    logging::init();
    let args = AppArgs::parse();
    core::app::run(args).await
}
