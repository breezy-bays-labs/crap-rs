// Fixture: flat match with many arms — discriminates cognitive from cyclomatic.
// Cognitive: base 1 + match +1 = 2 (flat, no nesting penalty on arms).
// Cyclomatic: base 1 + (11 arms - 1) = 11.

pub fn http_status_text(code: u16) -> &'static str {
    match code {
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        301 => "Moved Permanently",
        304 => "Not Modified",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        500 => "Internal Server Error",
        _ => "Unknown",
    }
}
