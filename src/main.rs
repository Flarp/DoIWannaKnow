#![feature(plugin, use_extern_macros, custom_derive)]
#![plugin(dotenv_macros)]
#![plugin(rocket_codegen)]
#[macro_use] extern crate diesel;
extern crate rocket;
extern crate rocket_contrib;
#[macro_use] extern crate diesel_codegen;
extern crate dotenv;
#[macro_use] extern crate serde_derive;
#[macro_use] extern crate serde_json;
extern crate rand;
extern crate byteorder;
extern crate r2d2;
extern crate r2d2_diesel;

use rocket_contrib::{ Template, Json };
use rocket::response::content::Html;
use rocket::response::status::Custom;
use rocket::State;
use rocket::http::Status;
use diesel::pg::PgConnection;
use diesel::Connection;
use diesel::prelude::*;
use dotenv::dotenv;
use rocket::request::Form;
use serde_json::json;
use rand::{ thread_rng, Rng };
use std::thread;
use std::time::{ SystemTime, Duration, UNIX_EPOCH };
use std::io::Read;
use byteorder::{ LittleEndian, ByteOrder };

type CustomErr = Custom<Template>;
type TemplateResponder = Result<Template, CustomErr>;
type DIWKPool = r2d2::Pool<r2d2_diesel::ConnectionManager<PgConnection>>;

enum DIWKError {
  DieselError(diesel::result::Error),
  DieselConnectionError(diesel::result::ConnectionError),
  NotFound,
  IncorrectPassword,
  NotFinished,
  AlreadyFinished,
  InvalidRequestLength,
  IOError(std::io::Error),
  NoAvailableConnections
}

trait DIWKErrorHack {
  fn get_diwk_error(self) -> DIWKError;
}

macro_rules! impl_diwk_hack {
  ($implementor:path, $($variant:path)*) => {
    impl DIWKErrorHack for $implementor {
      fn get_diwk_error(self) -> DIWKError {
        $($variant)*(self)
      }
    }
  }
}

impl_diwk_hack!(std::io::Error, DIWKError::IOError);
impl_diwk_hack!(diesel::result::ConnectionError, DIWKError::DieselConnectionError);
impl_diwk_hack!(diesel::result::Error, DIWKError::DieselError);
impl_diwk_hack!(DIWKError, );

macro_rules! diwk_try {
  ($test:expr, true) => {
    match $test {
      Ok(z) => z,
      Err(z) => return Err(handle_diwk_error(z.get_diwk_error()))
    }
  };
  ($test:expr, false) => {
    match $test {
      Ok(z) => z,
      Err(z) => return Err(z.get_diwk_error())
    }
  };
}

fn parse_bytes(thing: rocket::data::Data) -> Result<i64, DIWKError> {
  let mut bytes = thing.open();
  let mut buffer = [0; 8];
  diwk_try!(bytes.read(&mut buffer), false);
  let mut num: i64 = 0;
  num |= LittleEndian::read_u32(&mut buffer[0..4]) as i64;
  num <<= 32;
  num |= LittleEndian::read_u32(&mut buffer[4..8]) as i64;
  Ok(num)
}

const TWENTY_FOUR_HOURS: u64 = 24 * 60 * 60 * 1000;

diesel::infer_schema!("dotenv:DATABASE_URL");

fn get_rand() -> i32 {
  let mut random = thread_rng();
  random.gen_range(0, std::i32::MAX)
}

fn return_error<T: std::fmt::Display>(string: T) -> Template {
  Template::render("error", json!( { "error": format!("{}", string) } ))
}

fn get_chart_with_id(id: i32, connection: &PgConnection) -> Result<OpinionChartSQL, DIWKError> {
  let mut result = diwk_try!(opinioncharts::table.filter(opinioncharts::columns::id.eq(id)).load::<OpinionChartSQL>(connection), false);
  result.pop().ok_or(DIWKError::NotFound)
}

fn get_session(id: i32, connection: &PgConnection) -> Result<OpinionSessionQuery, DIWKError> {
  let mut result = diwk_try!(opinionsessions::table.filter(opinionsessions::columns::id.eq(id)).load::<OpinionSessionQuery>(connection), false);
  result.pop().ok_or(DIWKError::NotFound)
}

#[get("/")]
fn home() -> Html<&'static str> {
  Html(include_str!("index.html"))
}

fn handle_diwk_error(error: DIWKError) -> CustomErr {
  match error {
    DIWKError::NotFound => Custom(Status::NotFound, return_error("Not Found")),
    DIWKError::DieselError(x) => Custom(Status::InternalServerError, return_error(x)),
    DIWKError::IncorrectPassword => Custom(Status::Unauthorized, return_error("Wrong password")),
    DIWKError::NotFinished => Custom(Status::BadRequest, return_error("Game has not finished")),
    DIWKError::InvalidRequestLength => Custom(Status::BadRequest, return_error("Your request length is invalid")),
    DIWKError::DieselConnectionError(x) => Custom(Status::InternalServerError, return_error(x)),
    DIWKError::AlreadyFinished => Custom(Status::BadRequest, return_error("Game has already finished")),
    DIWKError::IOError(x) => Custom(Status::InternalServerError, return_error(x)),
    DIWKError::NoAvailableConnections => Custom(Status::ServiceUnavailable, return_error("There are currently no open SQL connections. Please refresh the page and try again."))
  }
}

fn in_common(id: i32, integer: i64, conn: &PgConnection) -> Result<Vec<String>, DIWKError> {
  let mut data = diwk_try!(get_chart_with_id(id, conn), false);
  let mut answers: Vec<String> = Vec::new();
  let mut mixed: i64 = integer.clone();
  while let Some(x) = data.opinions.pop() {
    if mixed & 1 == 1 {
      answers.push(x);
    }
    mixed >>= 1;
  }
  Ok(answers)
}

#[derive(FromForm)]
struct WritePass { write_pass: i32 }

#[get("/view/<id>?<write_password>")]
fn write_pass(id: i32, write_password: WritePass, pool: State<DIWKPool>) -> TemplateResponder {
  let connection = &*diwk_try!(get_connection((*pool).clone()), true);
  let result = diwk_try!(get_session(id, &connection), true);
  if result.write_pass == -1 {
    return Err(handle_diwk_error(DIWKError::AlreadyFinished))
  }
  if result.write_pass == write_password.write_pass {
    let chart = diwk_try!(get_chart_with_id(result.chart_id, &connection), true);
    Ok(Template::render("play", json!({ "title": chart.title, "description": chart.description, "opinions": chart.opinions, "password": result.write_pass, "max_checks": result.max_checks })))
  } else {
    Err(handle_diwk_error(DIWKError::IncorrectPassword))
  }
}

#[derive(FromForm)]
struct ReadPass { read_pass: i32 }

#[get("/view/<id>?<read_pass>", rank=2)]
fn read_pass(id: i32, read_pass: ReadPass, pool: State<DIWKPool>) -> TemplateResponder {
  let connection = &*diwk_try!(get_connection((*pool).clone()), true);
  let session = diwk_try!(get_session(id, &connection), true);
  if session.write_pass != -1 {
    return Err(handle_diwk_error(DIWKError::NotFinished));
  }
  if session.read_pass == read_pass.read_pass {
    let results = diwk_try!(in_common(session.chart_id, session.opinion, &connection), true);
    diwk_try!(diesel::delete(opinionsessions::table.filter(opinionsessions::columns::id.eq(session.id))).execute(connection), true);
    Ok(Template::render("results", json!({ "answers": results })))
  } else {
    Err(handle_diwk_error(DIWKError::IncorrectPassword))
  }
}

#[get("/search")]
fn search() -> Html<&'static str> {
  Html(include_str!("search.html"))
}

#[derive(FromForm)]
struct Keyword { query: String }

#[post("/search/keyword", data="<keyword>")]
fn search_from_keyword(keyword: Form<Keyword>, pool: State<DIWKPool>) -> TemplateResponder {
  let connection = &*diwk_try!(get_connection((*pool).clone()), true);
  let formatted = format!("%{}%", keyword.get().query);
  let results = diwk_try!(opinioncharts::table.filter(opinioncharts::columns::title.ilike(formatted)).load::<OpinionChartSQL>(connection), true);
  Ok(Template::render("search_results", json!({ "results": results })))
}

#[post("/view/<id>?<write_pass>", data="<bytes>")]
fn answer(write_pass: WritePass, bytes: rocket::data::Data, id: i32, pool: State<DIWKPool>) -> TemplateResponder {
  let connection = &*diwk_try!(get_connection((*pool).clone()), true);
  let inside = diwk_try!(parse_bytes(bytes), true);
  let result = diwk_try!(get_session(id, &connection), true);
  if result.write_pass == -1 {
    return Err(handle_diwk_error(DIWKError::AlreadyFinished))
  } else if result.write_pass != write_pass.write_pass {
    return Err(handle_diwk_error(DIWKError::IncorrectPassword))
  };
  if result.opinion.signum() == -1 {
    let combined = (result.opinion & inside) & std::i64::MAX;
    let strings = diwk_try!(in_common(result.chart_id, combined, &connection), true); 
    diwk_try!(diesel::update(opinionsessions::table.filter(opinionsessions::columns::id.eq(result.id))).set((opinionsessions::columns::write_pass.eq(-1), opinionsessions::columns::opinion.eq(combined))).get_result::<OpinionSessionQuery>(connection), true);
    Ok(Template::render("results", json!({ "answers": strings })))
  } else {
    let answers = diwk_try!(diesel::update(opinionsessions::table.filter(opinionsessions::columns::id.eq(result.id))).set((opinionsessions::columns::opinion.eq(inside | std::i64::MIN), opinionsessions::columns::read_pass.eq(get_rand()))).get_result::<OpinionSessionQuery>(connection), true);
    Ok(Template::render("answered", &answers))
  }
}

#[get("/create")]
fn create() -> Html<&'static str> {
  Html(include_str!("create.html"))
}

#[get("/session")]
fn start_game() -> Template {
  Template::render("session", json!({ "id": 1 }))
}

#[get("/session/<id>")]
fn start_game_with_id(id: i32) -> Template {
  Template::render("session", json!({ "id": id }))
}

#[post("/session", data = "<form>")]
fn actually_start_game<'a>(form: Form<OpinionSessionForm>, pool: State<DIWKPool>) -> Result<rocket::response::Response<'a>, CustomErr> {
  let mut value = form.into_inner();
  let connection = &*diwk_try!(get_connection((*pool).clone()), true);
  diwk_try!(get_chart_with_id(value.chart_id, &connection), true);
  value.write_pass = get_rand();
  let result = diwk_try!(diesel::insert(&value).into(opinionsessions::table).get_result::<OpinionSessionQuery>(connection), true);
  Ok(rocket::response::Response::build()
  .status(Status::SeeOther)
  .header(rocket::http::hyper::header::Location(format!("/view/{}?write_pass={}", result.id, value.write_pass)))
  .finalize())
}

#[post("/create", format="application/json", data="<upload>")]
fn post_create(upload: Json<OpinionChartJSON>, pool: State<DIWKPool>) -> TemplateResponder {
  let form = upload.into_inner();
  let connection = &*diwk_try!(get_connection((*pool).clone()), true);
  if form.opinions.len() > 63 {
    return Err(handle_diwk_error(DIWKError::InvalidRequestLength));
  }
  let x = diwk_try!(diesel::insert(&form).into(opinioncharts::table).get_result::<OpinionChartSQL>(connection), true);
  Ok(Template::render("created", &x))
}

fn get_connection(pool: DIWKPool) -> Result<r2d2::PooledConnection<r2d2_diesel::ConnectionManager<PgConnection>>, DIWKError> {
  let conn = diwk_try!(pool.try_get().ok_or(DIWKError::NoAvailableConnections), false);
  Ok(conn)
}

#[derive(Debug, Serialize, Deserialize, Insertable)]
#[table_name="opinioncharts"]
struct OpinionChartJSON {
  title: String,
  description: String,
  opinions: Vec<String>,
}

#[derive(Debug, Queryable, Serialize, Deserialize)]
struct OpinionChartSQL {
  id: i32,
  title: String,
  description: String,
  opinions: Vec<String>
}

#[derive(FromForm, Insertable)]
#[table_name="opinionsessions"]
struct OpinionSessionForm {
  chart_id: i32,
  max_checks: i16,
  write_pass: i32
}

#[derive(Debug, Queryable, Serialize, AsChangeset)]
#[table_name="opinionsessions"]
struct OpinionSessionQuery {
  id: i32,
  chart_id: i32,
  max_checks: i16,
  opinion: i64,
  read_pass: i32,
  write_pass: i32,
  creation_time: i64
}

fn main() {
  let pool = r2d2::Pool::new(r2d2::Config::default(), r2d2_diesel::ConnectionManager::<PgConnection>::new(dotenv!("DATABASE_URL"))).expect("FAILED TO CREATE POOL");
  thread::spawn(|| {
    
    let one_hour = Duration::new(60*60, 0);
    loop {
      while {
        match PgConnection::establish(dotenv!("DATABASE_URL")) {
          Ok(connection) => {
            let twenty_four_hours_ago = ((SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("unreachable really")
            .as_secs() * 1000) - (TWENTY_FOUR_HOURS)) as i64;
            match diesel::delete(opinionsessions::table.filter(opinionsessions::columns::creation_time.lt(twenty_four_hours_ago))).execute(&connection) {
              Ok(_) => (),
              Err(x) => {
                println!("{}", x);
                ()
              }
            }
          },
          Err(_) => ()
        }

      thread::sleep(one_hour);

      false

      } {}
    }
  });
  rocket::ignite()
  .mount("/", routes![home, create, post_create, start_game, start_game_with_id, actually_start_game, answer, search, search_from_keyword, read_pass, write_pass])
  .attach(Template::fairing())
  .manage(pool)
  .launch();
}
