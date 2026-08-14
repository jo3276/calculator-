//this will make all the backend route to share the same database 
// instead of making a new one in every function

use sqlx::PgPool;

#[derive(Clone)]
pub struct AppState {   
    pub db: PgPool,
} /* this will make a shared route for auxum. every request handeler will get appstate
and use state.db to talk postgres */