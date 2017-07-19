#![feature(plugin, use_extern_macros, custom_derive)]
#![plugin(rocket_codegen)]
#![plugin(dotenv_macros)]
//#![feature(trace_macros)]
//trace_macros!(true);

#[macro_use] extern crate diesel;
extern crate rocket;
extern crate rocket_contrib;
#[macro_use] extern crate diesel_codegen;
extern crate dotenv;
extern crate serde;
#[macro_use] extern crate serde_derive;
#[macro_use] extern crate serde_json;

use rocket_contrib::{ Template, Json };
use rocket::response::content::Html;
use rocket::response::status::Custom;
use rocket::response::Response;
use rocket::http::Status;
use rocket::http::hyper::header;
use diesel::pg::PgConnection;
use diesel::Connection;
use diesel::prelude::*;
use dotenv::dotenv;
use rocket::request::Form;
use serde_json::json;

type CustomErr = Custom<Template>;
type HtmlResponder = Result<Html<&'static str>, CustomErr>;
type RedirectResponder = Result<Response<'static>, CustomErr>;

enum DIWKError {
  DieselError(diesel::result::Error),
  NotFound,
}

const SESSION: &'static str = concat!("<html>\n<body>\n", include_str!("session.html"));
const INDEX: &'static str = include_str!("index.html");
const CREATE: &'static str = include_str!("create.html");
//const SESSION_FAIL: &'static str = concat!("<html>\n<body>\n", "<p>That ID does not exist. Please try another one.<p>\n", include_str!("session.html"));
//const INTERNAL_SESSION_ERROR: &'static str = concat!("<html>\n<body>\n", "<p>There was an internal error in the server. Please try again later</p>", include_str!("session.html"));

diesel::embed_migrations!("migrations");
diesel::infer_schema!("dotenv:DATABASE_URL");

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

fn redirect(link: String) -> Response<'static> {
  Response::build()
  .status(Status::SeeOther)
  .header(header::Location(link))
  .finalize()
}

fn in_common(result: &OpinionSessionQuery, integer: i64) -> Result<Vec<String>, DIWKError> {
  match get_chart_with_id(result.chart_id) {
    Ok(data) => {
      let mut answers: Vec<String> = Vec::new();
      let mut mixed: i64 = (result.opinion & integer) ^ std::i64::MIN;
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

#[get("/view/<id>")]
fn started(id: i32) -> Option<Template> {
  match get_session(id) {
    Ok(result) => {
      if result.done == true {
        
      }
      match get_chart_with_id(result.chart_id) {
        Ok(x) => Some(Template::render("play", &x)),
        Err(_) => None
      }
    },
    Err(_) => None
    /*
    Ok(results) => {
      match results.get(0) {
        Some(result) => {
          match get_chart_with_id(result.chart_id) {
            Ok(x) =>  {
              match x.get(0) {
                Some(x) => Some(Template::render("play", &x)),
                None => None
              }
            },
            Err(_) => None
          }
        },
        None => None
      }
    },
    Err(_) => None
    */
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

#[post("/view/<id>", format="application/json", data="<checks>")]
fn answer(checks: Json<Vec<bool>>, id: i32) -> Result<Template, Custom<Template>> {
  let inner = checks.into_inner();
  match get_session(id) {
    Err(error) => match error {
      DIWKError::NotFound => Err(Custom(Status::NotFound, return_error("There are no sessions that exist with that ID. Please try another one."))),
      DIWKError::DieselError(x) => Err(Custom(Status::InternalServerError, return_error(x)))
    },
    Ok(result) => {
      if result.done == true {
        return Err(Custom(Status::BadRequest, return_error("This game has already finished.")));
      };
      match integerify(inner, result.max_checks as usize) {
        None => Err(Custom(Status::BadRequest, return_error("You have selected over the maximum amount of checks. Please refresh and try again."))),
        Some(integer) => {
          let connection = start_connection();
          if result.opinion.signum() == -1 {
            match in_common(&result, integer) {
              Ok(strings) => match diesel::update(opinionsessions::table.filter(opinionsessions::columns::id.eq(result.id))).set(opinionsessions::columns::done.eq(true)).get_result::<OpinionSessionQuery>(&connection) {
                Ok(_) => Ok(Template::render("results", json!({ "answers": strings }))),
                Err(x) => Err(Custom(Status::InternalServerError, return_error(x)))
              },
              Err(err) => match err {
                DIWKError::DieselError(err) => Err(Custom(Status::InternalServerError, return_error(err))),
                DIWKError::NotFound => Err(Custom(Status::NotFound, return_error("Not Found. You messed something up bad.")))
              }
            }
                
          } else {
            match diesel::update(opinionsessions::table.filter(opinionsessions::columns::id.eq(result.id))).set((opinionsessions::columns::opinion.eq(integer))).get_result::<OpinionSessionQuery>(&connection) {
              Ok(_) => Ok(Template::render("answered", &result)),
              Err(x) => Err(Custom(Status::InternalServerError, return_error(x)))
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
fn start_game() -> Html<&'static str> {
  Html(SESSION)
}

#[post("/session", data = "<form>")]
fn actually_start_game(form: Form<OpinionSessionForm>) -> RedirectResponder {
  let value = form.into_inner();
  match get_chart_with_id(value.chart_id) {
    Ok(_) => {
      let connection = start_connection();
      match diesel::insert(&value).into(opinionsessions::table).get_result::<OpinionSessionQuery>(&connection) {
        Ok(x) => Ok(redirect(format!("/view/{}", x.id))),
        Err(x) => Err(Custom(Status::InternalServerError, return_error(x)))
      } 

    },
    Err(_) => Err(Custom(Status::UnprocessableEntity, return_error("Not Found")))
  } 
}


#[post("/create", format="application/json", data="<upload>")]
fn post_create(upload: Json<OpinionChartJSON>) -> Result<Template, Custom<Template>> {
  let form = upload.into_inner();
  let connection = start_connection();
  println!("{:?}", form);
  match diesel::insert(&form).into(opinioncharts::table).get_result::<OpinionChartSQL>(&connection) {
    Ok(x) => Ok(Template::render("created", &x)),
    Err(x) => Err(Custom(Status::InternalServerError, return_error(x))) 
  }

}

fn start_connection() -> PgConnection {
  let connection = PgConnection::establish(dotenv!("DATABASE_URL")).ok().unwrap();
  embedded_migrations::run(&connection).ok().unwrap();
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
}

#[derive(Debug, Queryable, Serialize, AsChangeset)]
#[table_name="opinionsessions"]
struct OpinionSessionQuery {
  id: i32,
  chart_id: i32,
  max_checks: i16,
  opinion: i64,
  done: bool
}

fn main() {


    dotenv().ok();
    println!("{}", integerify(vec![true, false, true, true], 4).unwrap());
    rocket::ignite()
    .mount("/", routes![home, started, create, post_create, start_game, actually_start_game, answer])
    .attach(Template::fairing())
    .launch();
}
