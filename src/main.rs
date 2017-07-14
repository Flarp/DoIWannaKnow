#![feature(plugin, use_extern_macros)]
#![plugin(rocket_codegen)]
#![plugin(dotenv_macros)]

#[macro_use] extern crate diesel;
extern crate rocket;
extern crate rocket_contrib;
#[macro_use] extern crate diesel_codegen;
extern crate dotenv;
extern crate serde;
#[macro_use] extern crate serde_derive;

use rocket_contrib::Template;
use rocket::response::content::HTML;
use diesel::pg::PgConnection;
use diesel::Connection;
use diesel::prelude::*;
use dotenv::dotenv;

diesel::embed_migrations!("migrations");
diesel::infer_schema!("dotenv:DATABASE_URL");

#[get("/")]
fn home() -> HTML<&'static str> {
  HTML(include_str!("index.html"))
}

/*#[derive(Insertable, Queryable)]
#[table_name="OPINIONSESSIONS"]
struct OpinionSession {
  id: u64,
}
*/

#[derive(Queryable, Debug)]
struct OpinionChart {
  id: i64,
  title: String,
  description: String,
  opinions: Vec<String>,
}

fn main() {
    dotenv().ok();
    
    let connection = PgConnection::establish(dotenv!("DATABASE_URL")).ok().unwrap();
    embedded_migrations::run(&connection).ok().unwrap();
    println!("{}", dotenv!("DATABASE_URL"));
    let results = opinioncharts::table.filter(opinioncharts::columns::id.eq(4)).load::<OpinionChart>(&connection).expect("ERR");
    println!("{:?}", results);
    rocket::ignite().mount("/", routes![home]).launch();
}
