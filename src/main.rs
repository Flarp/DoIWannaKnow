#![feature(plugin, use_extern_macros, custom_derive)]
#![plugin(dotenv_macros)]
#![plugin(rocket_codegen)]

#[macro_use] extern crate diesel;
extern crate rocket;
extern crate rocket_contrib;
#[macro_use] extern crate diesel_codegen;
extern crate dotenv;
extern crate serde;
#[macro_use] extern crate serde_derive;
#[macro_use] extern crate serde_json;
extern crate rand;

use rocket_contrib::{ Template, Json };
use rocket::response::content::Html;
use rocket::response::status::Custom;
use rocket::http::Status;
use diesel::pg::PgConnection;
use diesel::Connection;
use diesel::prelude::*;
use dotenv::dotenv;
use rocket::request::Form;
use serde_json::json;
use rand::{thread_rng, Rng};
use std::thread;
use std::time::{ SystemTime, Duration, UNIX_EPOCH };

type CustomErr = Custom<Template>;
type TemplateResponder = Result<Template, CustomErr>;

enum DIWKError {
  DieselError(diesel::result::Error),
  NotFound,
  IncorrectPassword,
  NotFinished
}

const INDEX: &'static str = include_str!("index.html");
const CREATE: &'static str = include_str!("create.html");
const SEARCH: &'static str = include_str!("search.html");
const TWENTY_FOUR_HOURS: u64 = 24 * 60 * 60 * 1000;

diesel::infer_schema!("dotenv:DATABASE_URL");

fn get_rand() -> i32 {
  let mut random = thread_rng();
  random.gen_range(0, std::i32::MAX)
}

fn return_error<T: std::fmt::Display>(string: T) -> Template {
  Template::render("error", json!( { "error": format!("{}", string) } ))
}

fn get_chart_with_id(id: i32) -> Result<OpinionChartSQL, DIWKError> {
  let connection = start_connection();
  let result = opinioncharts::table.filter(opinioncharts::columns::id.eq(id)).load::<OpinionChartSQL>(&connection);
  match result {
    Ok(mut x) => {
      match x.pop() {
        Some(x) => Ok(x),
        None => Err(DIWKError::NotFound)
      }
    },
    Err(x) => Err(DIWKError::DieselError(x))
  }
}

fn get_session(id: i32) -> Result<OpinionSessionQuery, DIWKError> {
  let connection = start_connection();
  match opinionsessions::table.filter(opinionsessions::columns::id.eq(id)).load::<OpinionSessionQuery>(&connection) {
    Err(x) => Err(DIWKError::DieselError(x)),
    Ok(mut x) => {
      match x.pop() {
        Some(x) => Ok(x),
        None => Err(DIWKError::NotFound)
      }
    } 
  }
}

#[get("/")]
fn home() -> Html<&'static str> {
  Html(INDEX)
}

fn handle_diwk_error(error: DIWKError) -> Custom<Template> {
  match error {
    DIWKError::NotFound => Custom(Status::NotFound, return_error("Not Found")),
    DIWKError::DieselError(x) => Custom(Status::InternalServerError, return_error(x)),
    DIWKError::IncorrectPassword => Custom(Status::Unauthorized, return_error("Wrong password")),
    DIWKError::NotFinished => Custom(Status::BadRequest, return_error("Game has not finished"))
  }
}

fn in_common(id: i32, integer: i64) -> Result<Vec<String>, DIWKError> {
  match get_chart_with_id(id) {
    Ok(data) => {
      let mut answers: Vec<String> = Vec::new();
      let mut mixed: i64 = integer.clone();
      for x in data.opinions.iter().rev() {
        if mixed & 1 == 1 {
          answers.push(x.clone());
        }
        mixed >>= 1;
      }
      Ok(answers)

    },
    Err(err) => Err(err)
  }

}

#[derive(FromForm)]
struct Write { write_pass: i32 }

#[get("/view/<id>?<write_password>")]
fn write_pass(id: i32, write_password: Write) -> TemplateResponder {
  match get_session(id) {
    Ok(result) => {
      if result.write_pass == write_password.write_pass {
        match get_chart_with_id(result.chart_id) {
          Ok(x) => Ok(Template::render("play", json!({ "title": x.title, "description": x.description, "opinions": x.opinions, "password": result.write_pass, "max_checks": result.max_checks }))),
          Err(x) => Err(handle_diwk_error(x))
        }
      } else {
        Err(handle_diwk_error(DIWKError::IncorrectPassword))
      }
    },
    Err(err) => Err(handle_diwk_error(err))
  }
}

#[derive(FromForm)]
struct Read { read_pass: i32 }

#[get("/view/<id>?<read_pass>", rank=2)]
fn read_pass(id: i32, read_pass: Read) -> TemplateResponder {
  match get_session(id) {
    Ok(session) => {
      if session.write_pass != -1 {
        return Err(handle_diwk_error(DIWKError::NotFinished));
      }
      if session.read_pass == read_pass.read_pass {
        match in_common(session.chart_id, session.opinion) {
          Ok(results) => {
            let connection = start_connection();
            match diesel::delete(opinionsessions::table.filter(opinionsessions::columns::id.eq(session.id))).execute(&connection) {
              Ok(_) => Ok(Template::render("results", json!({ "answers": results }))),
              Err(x) => Err(handle_diwk_error(DIWKError::DieselError(x)))
            }
          },
          Err(x) => Err(handle_diwk_error(x))
        }
      } else {
        Err(handle_diwk_error(DIWKError::IncorrectPassword))
      }
    },
    Err(x) => Err(handle_diwk_error(x))
  }
}

//makeshift bitfield
fn integerify(bools: Vec<bool>, length: usize) -> Option<i64> {
  let mut real_length: usize = 0;
  let mut num: i64 = 0;
  for &x in bools.iter() {
    num <<= 1;
    if x == true {
      real_length += 1;
      num |= 1;
    };
  };
  
  if length >= real_length { Some(num | std::i64::MIN) } else { None }

}

#[get("/search")]
fn search() -> Html<&'static str> {
  Html(SEARCH)
}

#[derive(FromForm)]
struct Keyword { query: String }

#[post("/search/keyword", data="<keyword>")]
fn search_from_keyword(keyword: Form<Keyword>) -> TemplateResponder {
  let connection = start_connection();
  let formatted = format!("%{}%", keyword.get().query);
  match opinioncharts::table.filter(opinioncharts::columns::title.ilike(formatted)).load::<OpinionChartSQL>(&connection) {
    Ok(x) => Ok(Template::render("search_results", json!({ "results": x }))),
    Err(x) => Err(handle_diwk_error(DIWKError::DieselError(x)))
  }
}

/*
#[derive(FromForm)]
struct ID { number: i32 }

#[post("/search/id", data="<id_search>")]
fn search_from_id(id_search: Form<ID>) -> TemplateResponder {
  let connection = start_connection();
  match opinioncharts::table.filter(opinioncharts::columns::id.eq(id_search.get().number)).load::<OpinionChartSQL>(&connection) {
    Ok(x) => Ok(Template::render("search_results", json!({ "results": x }))),
    Err(x) => Err(handle_diwk_error(DIWKError::DieselError(x)))

  }
}
*/

#[derive(Deserialize)]
struct PlaySubmission {
  result: Vec<bool>,
  write_pass: i32
}

#[post("/view/<id>", format="application/json", data="<checks>")]
fn answer(checks: Json<PlaySubmission>, id: i32) -> TemplateResponder {
  let inside = checks.into_inner();
  match get_session(id) {
    Err(error) => Err(handle_diwk_error(error)),
    Ok(result) => {
      if result.write_pass == -1 {
        return Err(Custom(Status::BadRequest, return_error("This game has already finished.")));
      } else if result.write_pass != inside.write_pass {
        return Err(handle_diwk_error(DIWKError::IncorrectPassword))
      };
      match integerify(inside.result, result.max_checks as usize) {
        None => Err(Custom(Status::BadRequest, return_error("You have selected over the maximum amount of checks. Please refresh and try again."))),
        Some(integer) => {
          let connection = start_connection();
          if result.opinion.signum() == -1 {
            let combined = (result.opinion & integer) & std::i64::MAX;
            match in_common(result.chart_id, combined) {

              Ok(strings) => match diesel::update(opinionsessions::table.filter(opinionsessions::columns::id.eq(result.id))).set((opinionsessions::columns::write_pass.eq(-1), opinionsessions::columns::opinion.eq(combined))).get_result::<OpinionSessionQuery>(&connection) {
                Ok(_) => Ok(Template::render("results", json!({ "answers": strings }))),
                Err(x) => Err(handle_diwk_error(DIWKError::DieselError(x)))
              },
              Err(err) => Err(handle_diwk_error(err))
            }
                
          } else {
            match diesel::update(opinionsessions::table.filter(opinionsessions::columns::id.eq(result.id))).set((opinionsessions::columns::opinion.eq(integer), opinionsessions::columns::read_pass.eq(get_rand()))).get_result::<OpinionSessionQuery>(&connection) {
              Ok(x) => Ok(Template::render("answered", &x)),
              Err(x) => Err(handle_diwk_error(DIWKError::DieselError(x)))
            }
          } 
        }
      }
    }
  }
}

#[get("/create")]
fn create() -> Html<&'static str> {
  Html(CREATE)
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
fn actually_start_game(form: Form<OpinionSessionForm>) -> Result<rocket::response::Response, Custom<Template>> {
  let mut value = form.into_inner();
  match get_chart_with_id(value.chart_id) {
    Ok(_) => {
      let connection = start_connection();
      value.write_pass = get_rand();
      match diesel::insert(&value).into(opinionsessions::table).get_result::<OpinionSessionQuery>(&connection) {
        Ok(x) => {
          Ok(rocket::response::Response::build()
          .status(Status::SeeOther)
          .header(rocket::http::hyper::header::Location(format!("/view/{}?write_pass={}", x.id, value.write_pass)))
          .finalize())
        },
        Err(x) => Err(handle_diwk_error(DIWKError::DieselError(x)))
      } 

    },
    Err(x) => Err(handle_diwk_error(x))
  } 
}

#[post("/create", format="application/json", data="<upload>")]
fn post_create(upload: Json<OpinionChartJSON>) -> TemplateResponder {
  let form = upload.into_inner();
  let connection = start_connection();
  if form.opinions.len() > 63 {
    return Err(Custom(Status::BadRequest, return_error("The form provided is too long.")));
  }
  for z in form.opinions.iter() {
    if z.len() > 127 {
      return Err(Custom(Status::BadRequest, return_error("One or more form inputs are too long.")));
    }
  }
  match diesel::insert(&form).into(opinioncharts::table).get_result::<OpinionChartSQL>(&connection) {
    Ok(x) => Ok(Template::render("created", &x)),
    Err(x) => Err(handle_diwk_error(DIWKError::DieselError(x)))
  }

}

fn start_connection() -> PgConnection {
  let connection = PgConnection::establish(dotenv!("DATABASE_URL")).ok().unwrap();
  connection
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
  thread::spawn(move || {
    
    let one_hour = Duration::new(60*60, 0);
    loop {
      while {
        let connection = start_connection();
        let twenty_four_hours_ago = ((SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("unreachable really")
        .as_secs() * 1000) - (TWENTY_FOUR_HOURS)) as i64;

        match diesel::delete(opinionsessions::table.filter(opinionsessions::columns::creation_time.lt(twenty_four_hours_ago))).execute(&connection) {
          Ok(_) => 0,
          Err(x) => {
            println!("{}", x);
            0
          }
        };
        false
      } {}

      thread::sleep(one_hour);

    }
  });
  dotenv().ok();
  rocket::ignite()
  .mount("/", routes![home, create, post_create, start_game, start_game_with_id, actually_start_game, answer, search, search_from_keyword, search_from_id, read_pass, write_pass])
  .attach(Template::fairing())
  .launch();
}
