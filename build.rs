use actix_web_static_files::resource_dir;

fn main() -> std::io::Result<()> {
    resource_dir("./static").build()
}