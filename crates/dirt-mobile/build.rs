use std::env;
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const QUICK_CAPTURE_WIDGET_XML: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<appwidget-provider xmlns:android="http://schemas.android.com/apk/res/android"
    android:minWidth="120dp"
    android:minHeight="48dp"
    android:updatePeriodMillis="0"
    android:initialLayout="@android:layout/simple_list_item_1"
    android:resizeMode="horizontal|vertical"
    android:widgetCategory="home_screen" />
"#;

fn main() {
    println!("cargo:rerun-if-env-changed=WRY_ANDROID_KOTLIN_FILES_OUT_DIR");

    if let Err(error) = write_android_widget_resources() {
        println!("cargo:warning=failed to generate Android widget metadata: {error}");
    }
}

fn write_android_widget_resources() -> io::Result<()> {
    let Some(kotlin_out_dir) = env::var_os("WRY_ANDROID_KOTLIN_FILES_OUT_DIR") else {
        return Ok(());
    };

    let Some(main_dir) = find_android_main_dir(Path::new(&kotlin_out_dir)) else {
        return Ok(());
    };

    let xml_dir = main_dir.join("res").join("xml");
    fs::create_dir_all(&xml_dir)?;
    fs::write(
        xml_dir.join("dirt_quick_capture_widget_info.xml"),
        QUICK_CAPTURE_WIDGET_XML,
    )?;

    Ok(())
}

fn find_android_main_dir(path: &Path) -> Option<PathBuf> {
    path.ancestors().find_map(|ancestor| {
        let parent = ancestor.parent()?;
        if ancestor.file_name() == Some(OsStr::new("main"))
            && parent.file_name() == Some(OsStr::new("src"))
        {
            Some(ancestor.to_path_buf())
        } else {
            None
        }
    })
}
