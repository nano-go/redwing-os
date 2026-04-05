use std::{
    fs::{self, File},
    path::Path,
};

use dialoguer::Confirm;

use crate::{custom_err, error::Result, img_file::ImageFile, CmdArgs};

pub fn make_image_file(args: &CmdArgs) -> Result<()> {
    let path = args.img_file.as_ref();
    if !check_and_create_file(&path, args)? {
        return Ok(());
    }

    let img_file = ImageFile::make(path, args)?;
    img_file.make_basic_dirs()?;
    img_file.sync_all()?;
    Ok(())
}

fn check_and_create_file(path: &Path, args: &CmdArgs) -> Result<bool> {
    if !fs::exists(path)? {
        create_img_file(path, args.size as u64)?;
        return Ok(true);
    }

    let metadata = fs::metadata(path)?;
    if !metadata.is_file() {
        return Err(custom_err!(
            "The image file is not a regular file: {}",
            path.display()
        ));
    }

    let overwrite = args.force
        || Confirm::new()
            .with_prompt("The image file already exist. Do you want overwrite it?")
            .interact()
            .unwrap();
    if overwrite {
        fs::remove_file(path)?;
        create_img_file(path, args.size as u64)?;
    }
    Ok(overwrite)
}

fn create_img_file(path: &Path, size: u64) -> Result<()> {
    let file = File::create(path)?;
    file.set_len(size)?;
    Ok(())
}

pub fn cp_dir<P: AsRef<Path>, D: AsRef<Path>>(args: &CmdArgs, src: P, dst: D) -> Result<()> {
    let src = src.as_ref();
    let dst = dst.as_ref();

    let img_file = ImageFile::open(&args.img_file)?;

    for dirent in fs::read_dir(src)? {
        let dirent = dirent?;
        if !dirent.file_type()?.is_file() {
            continue;
        }
        let path = dirent.path();
        let buf = fs::read(&path)?;
        let bin_name = path.file_name().unwrap();
        img_file.create_file(dst, bin_name.to_str().unwrap(), &buf)?;
    }
    img_file.sync_all()
}
