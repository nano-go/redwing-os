use args::CmdArgs;
use clap::Parser;
use error::Result;

mod args;
mod error;
mod img_file;
mod mkfs;

extern crate alloc;

fn main() -> Result<()> {
    let args = CmdArgs::parse();

    if args.mkfs {
        mkfs::make_image_file(&args)?;
    }

    if let Some(ref user_bin_path) = args.user_bin_path {
        mkfs::cp_dir(&args, user_bin_path, "/bin")?;
    }

    if !args.mkfs && args.user_bin_path.is_none() {
        println!("No operation specified. Use --help for usage information.");
    }

    Ok(())
}
