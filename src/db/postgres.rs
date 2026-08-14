// so this postgress.rs will is used to connect your Rust backend to the PostgreSQL database.

use sqlx::PgPool;  //import pgpool which is a connection pool from sqlx

pub async fn connect_db() -> PgPool { //this connect to database and return a reusable function
    dotenvy::dotenv().ok(); //this will load variable like database url from .env file

    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set"); /*
    thi will read database url from .env is miss server will crash */

    PgPool::connect(&database_url) //connect to postgress
        .await
        .expect("Failed to connect to database") 
}
