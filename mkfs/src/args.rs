use clap::Parser;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct CmdArgs {
    /// Create a new filesystem (short flag -m)
    #[arg(short = 'm', long)]
    pub mkfs: bool,

    /// Ignores exitent image file and prompt.
    #[arg(short = 'f', long)]
    pub force: bool,

    /// The size of the img file.
    #[arg(short = 's', long, default_value = "60M", value_parser=size_parser)]
    pub size: usize,

    /// The size of inodes.
    #[arg(short = 'i', long, default_value = "512K", value_parser=size_parser)]
    pub inode_size: usize,

    /// Specifies the directory path that contains user executable files.
    /// 'mkfs' will move theme into '/bin' in the file system you are created.
    #[arg(short = 'b', long)]
    pub user_bin_path: Option<String>,

    /// Image file to operate on.
    pub img_file: String,
}

fn size_parser(size: &str) -> Result<usize, String> {
    let multipler = match size.chars().last() {
        Some('K') | Some('k') => 1024,
        Some('M') | Some('m') => 1024 * 1024,
        Some('G') | Some('g') => 1024 * 1024 * 1024,
        Some('0'..='9') | Some('B') | Some('b') => 1,
        _ => return Err(format!("Invalid size format: {}", size)),
    };
    let result: usize = size[..size.len() - 1]
        .parse()
        .map_err(|_| format!("Invalid size format: {}", size))?;
    Ok(result * multipler)
}
