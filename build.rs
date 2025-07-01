fn main() {
    dotenvy::dotenv().ok();   // initialize environment
    
    // If DATABASE_URL is set in .env, make it available to `env!()`
    if let Ok(db_url) = std::env::var("DATABASE_URL") {
        println!("cargo:rustc-env=DATABASE_URL={}", db_url);
    }
}

