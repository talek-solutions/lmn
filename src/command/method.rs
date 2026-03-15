// TODO: remove `clap::ValueEnum` when splitting into workspace crates.
// At that point, define a CLI-side type and convert to this one in the adapter.
#[derive(clap::ValueEnum, Clone, Copy, Debug)]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Patch,
    Delete,
}

impl HttpMethod {
    pub fn as_str(self) -> &'static str {
        match self {
            HttpMethod::Get    => "GET",
            HttpMethod::Post   => "POST",
            HttpMethod::Put    => "PUT",
            HttpMethod::Patch  => "PATCH",
            HttpMethod::Delete => "DELETE",
        }
    }
}
