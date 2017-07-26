#![feature(plugin, use_extern_macros)]
#![plugin(dotenv_macros)]
extern crate diesel;
extern crate dotenv;

use dotenv::dotenv;
use diesel::Connection;

fn main() {
  dotenv().ok();
  let connection = diesel::pg::PgConnection::establish(dotenv!("DATABASE_URL")).ok().unwrap();
  diesel::migrations::run_pending_migrations(&connection).ok();
  
}
