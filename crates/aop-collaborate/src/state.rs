//! What every handler shares, mounted once as actix `app_data`.

use std::sync::Arc;

use sea_orm::DatabaseConnection;

use crate::auth::IdpClient;
use crate::config::Config;
use crate::live::Hub;

pub struct AppState {
    pub db: DatabaseConnection,
    pub idp: IdpClient,
    /// Live connections, per project. In process, which is the honest limit
    /// of this design: two instances behind a load balancer would each only
    /// broadcast to their own clients. Clients still converge, because the
    /// log in Postgres is the truth and a reconnect catches up from it, but
    /// live editing across instances needs a shared bus and does not have one
    /// yet.
    pub hub: Arc<Hub>,
    pub config: Arc<Config>,
}
