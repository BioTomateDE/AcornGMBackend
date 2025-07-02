use rocket::http::Status;
use rocket::Request;
use rocket_dyn_templates::{context, Template};
use crate::{respond_err, ApiResponse};


#[catch(404)]
pub fn html_catch_404(req: &Request) -> Template {
    Template::render("not_found", context!{
        url_path: req.uri().path().as_str()
    })
}

#[catch(404)]
pub fn api_catch_404(req: &Request) -> ApiResponse {
    Err(respond_err(Status::NotFound, &format!("Unknown URL: {}", req.uri().path().as_str())))
}

#[catch(422)]
pub fn api_catch_422() -> ApiResponse {
    Err(respond_err(Status::UnprocessableEntity, "ur json is fucked"))
}

#[catch(429)]
pub fn api_catch_429() -> ApiResponse {
    Err(respond_err(Status::TooManyRequests, "Too many requests!"))
}

