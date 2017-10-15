#![feature(plugin, use_extern_macros, custom_derive, const_fn)]
#![recursion_limit="128"]
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
extern crate r2d2;
extern crate r2d2_diesel;

use rocket_contrib::Template;
use rocket::response::content::Html;
use rocket::response::status::Custom;
use rocket::State;
use rocket::http::Status;
use diesel::pg::PgConnection;
use diesel::Connection;
use diesel::prelude::*;
use rocket::request::Form;
use serde_json::json;
use rand::{ thread_rng, Rng };
use std::thread;
use std::time::{ SystemTime, Duration, UNIX_EPOCH };

type CustomErr = Custom<Template>;
type TemplateResponder = Result<Template, DIWKError>;
type DIWKPool = r2d2::Pool<r2d2_diesel::ConnectionManager<PgConnection>>;

#[derive(Debug)]
enum DIWKError {
  DieselError(diesel::result::Error),
  NotFound,
  IncorrectPassword,
  NotFinished,
  AlreadyFinished,
  BadRequest,
  NoAvailableConnections
}

fn handle_diwk_error(error: DIWKError) -> CustomErr {
  match error {
    DIWKError::NotFound => Custom(Status::NotFound, return_error("Not Found")),
    DIWKError::DieselError(x) => Custom(Status::InternalServerError, return_error(x)),
    DIWKError::IncorrectPassword => Custom(Status::Unauthorized, return_error("Wrong password")),
    DIWKError::NotFinished => Custom(Status::BadRequest, return_error("Game has not finished")),
    DIWKError::BadRequest => Custom(Status::BadRequest, return_error("Bad request")),
    DIWKError::AlreadyFinished => Custom(Status::BadRequest, return_error("Game has already finished")),
    DIWKError::NoAvailableConnections => Custom(Status::ServiceUnavailable, return_error("There are currently no open SQL connections. Please refresh the page and try again."))
  }
}

impl<'r> rocket::response::Responder<'r> for DIWKError {
  fn respond_to(self, req: &rocket::request::Request) -> rocket::response::Result<'r> {
    handle_diwk_error(self).respond_to(req)
  }
}

trait DIWKErrorHack {
  fn get_diwk_error(self) -> DIWKError;
}

fn parse_rawstr(thing: String) -> Result<i64, DIWKError> {
  let mut num: i64 = 0;
  let mut count = 0;
  for (key, _) in rocket::request::FormItems::from(&thing[..]) {
    if count == 63 {
      return Err(DIWKError::BadRequest);
    }
    let decode = key.parse::<u32>().map_err(|_| DIWKError::BadRequest)?;
    let temp_num = 1i64.checked_shl(decode).ok_or(DIWKError::BadRequest)?;
    num |= temp_num;
    count += 1;
  }
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
  let mut result = opinioncharts::table.filter(opinioncharts::columns::id.eq(id)).load::<OpinionChartSQL>(connection).map_err(|e| DIWKError::DieselError(e))?;
  result.pop().ok_or(DIWKError::NotFound)
}

fn get_session(id: i32, connection: &PgConnection) -> Result<OpinionSessionQuery, DIWKError> {
  let mut result = opinionsessions::table.filter(opinionsessions::columns::id.eq(id)).load::<OpinionSessionQuery>(connection).map_err(|e| DIWKError::DieselError(e))?;
  result.pop().ok_or(DIWKError::NotFound)
}

#[get("/")]
fn home() -> Html<&'static str> {
  Html(include_str!("index.html"))
}

fn in_common(id: i32, integer: i64, conn: &PgConnection) -> Result<Vec<String>, DIWKError> {
  let mut data = get_chart_with_id(id, conn)?;
  let mut answers: Vec<String> = Vec::new();
  let mut mixed: i64 = integer.clone();
  data.opinions.reverse();
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
  let connection = &*(get_connection((*pool).clone()))?;
  let result = get_session(id, &connection)?;
  if result.write_pass == -1 {
    return Err(DIWKError::AlreadyFinished)
  }
  if result.write_pass == write_password.write_pass {
    let chart = get_chart_with_id(result.chart_id, &connection)?;
    Ok(Template::render("play", json!({ "title": chart.title, "description": chart.description, "opinions": chart.opinions, "password": result.write_pass, "max_checks": result.max_checks })))
  } else {
    Err(DIWKError::IncorrectPassword)
  }
}

#[derive(FromForm)]
struct ReadPass { read_pass: i32 }

#[get("/view/<id>?<read_pass>", rank=2)]
fn read_pass(id: i32, read_pass: ReadPass, pool: State<DIWKPool>) -> TemplateResponder {
  let connection = &*(get_connection((*pool).clone()))?;
  let session = get_session(id, &connection)?;
  if session.write_pass != -1 {
    return Err(DIWKError::NotFinished);
  }
  if session.read_pass == read_pass.read_pass {
    let results = in_common(session.chart_id, session.opinion, &connection)?;
    diesel::delete(opinionsessions::table.filter(opinionsessions::columns::id.eq(session.id))).execute(connection).map_err(|e| DIWKError::DieselError(e))?;
    Ok(Template::render("results", json!({ "answers": results })))
  } else {
    Err(DIWKError::IncorrectPassword)
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
  let connection = &*(get_connection((*pool).clone()))?;
  let formatted = format!("%{}%", keyword.get().query);
  let results = opinioncharts::table.filter(opinioncharts::columns::title.ilike(formatted)).load::<OpinionChartSQL>(connection).map_err(|e| DIWKError::DieselError(e))?;
  Ok(Template::render("search_results", json!({ "results": results })))
}

#[post("/view/<id>?<write_pass>", data="<string>")]
fn answer(string: String, id: i32, pool: State<DIWKPool>, write_pass: WritePass) -> TemplateResponder {
  let connection = &*(get_connection((*pool).clone()))?;
  let inside = parse_rawstr(string)?;
  let result = get_session(id, &connection)?;
  if result.write_pass == -1 {
    return Err(DIWKError::AlreadyFinished)
  } else if result.write_pass != write_pass.write_pass {
    return Err(DIWKError::IncorrectPassword)
  };
  if result.opinion.signum() == -1 {
    let combined = (result.opinion & inside) & std::i64::MAX;
    let strings = in_common(result.chart_id, combined, &connection)?; 
    diesel::update(opinionsessions::table.filter(opinionsessions::columns::id.eq(result.id))).set((opinionsessions::columns::write_pass.eq(-1), opinionsessions::columns::opinion.eq(combined))).get_result::<OpinionSessionQuery>(connection).map_err(|e| DIWKError::DieselError(e))?;
    Ok(Template::render("results", json!({ "answers": strings })))
  } else {
    let answers = diesel::update(opinionsessions::table.filter(opinionsessions::columns::id.eq(result.id))).set((opinionsessions::columns::opinion.eq(inside | std::i64::MIN), opinionsessions::columns::read_pass.eq(get_rand()))).get_result::<OpinionSessionQuery>(connection).map_err(|e| DIWKError::DieselError(e))?;
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
fn actually_start_game<'a>(form: Form<OpinionSessionForm>, pool: State<DIWKPool>) -> Result<rocket::response::Response<'a>, DIWKError> {
  let mut value = form.into_inner();
  let connection = &*(get_connection((*pool).clone()))?;
  get_chart_with_id(value.chart_id, &connection)?;
  value.write_pass = get_rand();
  let result = diesel::insert(&value).into(opinionsessions::table).get_result::<OpinionSessionQuery>(connection).map_err(|e| DIWKError::DieselError(e))?;
  Ok(rocket::response::Response::build()
  .status(Status::SeeOther)
  .header(rocket::http::hyper::header::Location(format!("/view/{}?write_pass={}", result.id, value.write_pass)))
  .finalize())
}

#[post("/create", data="<upload>")]
fn post_create(upload: Form<OpinionChartPost>, pool: State<DIWKPool>) -> TemplateResponder {
  let form = upload.into_inner();
  let insert = OpinionChartInsert { title: form.title, description: form.description, opinions: form.opinions.split("\n").map(|z| z.to_string()).collect() };
  let connection = &*(get_connection((*pool).clone()))?;
  if insert.opinions.len() > 63 || insert.opinions.iter().any(|z| z.len() > 127) {
    return Err(DIWKError::BadRequest);
  }
  let x = diesel::insert(&insert).into(opinioncharts::table).get_result::<OpinionChartSQL>(connection).map_err(|e| DIWKError::DieselError(e))?;
  Ok(Template::render("created", &x))
}

fn get_connection(pool: DIWKPool) -> Result<r2d2::PooledConnection<r2d2_diesel::ConnectionManager<PgConnection>>, DIWKError> {
  let conn = pool.try_get().ok_or(DIWKError::NoAvailableConnections)?;
  Ok(conn)
}

#[derive(Debug, Serialize, Deserialize, FromForm)]
struct OpinionChartPost {
  title: String,
  description: String,
  opinions: String,
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

#[derive(Insertable)]
#[table_name="opinioncharts"]
struct OpinionChartInsert {
  title: String,
  description: String,
  opinions: Vec<String>
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
