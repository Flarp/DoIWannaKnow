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

type CustomErr = Custom<Html<&'static str>>;
type HtmlResponder = Result<Html<&'static str>, CustomErr>;
type RedirectResponder = Result<Response<'static>, CustomErr>;

enum DIWKError {
  DieselError(diesel::result::Error),
  NotFound,
}

const SESSION: &'static str = concat!("<html>\n<body>\n", include_str!("session.html"));
const INDEX: &'static str = include_str!("index.html");
const CREATE: &'static str = include_str!("create.html");
const SESSION_FAIL: &'static str = concat!("<html>\n<body>\n", "<p>That ID does not exist. Please try another one.<p>\n", include_str!("session.html"));
const INTERNAL_SESSION_ERROR: &'static str = concat!("<html>\n<body>\n", "<p>There was an internal error in the server. Please try again later</p>", include_str!("session.html"));

diesel::embed_migrations!("migrations");
diesel::infer_schema!("dotenv:DATABASE_URL");

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

#[get("/play/<id>")]
fn started(id: i32) -> Option<Template> {
  match get_session(id) {
    Ok(result) => {
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
  if length != bools.len() {
    return None;
  }
  let mut num: i64 = 0;
  for &x in bools.iter() {
    num <<= 1;
    if x == true {
      num |= 1;
    };
  };
  Some(num | std::i64::MIN)

}

#[post("/play/<id>", format="application/json", data="<checks>")]
fn answer(checks: Json<Vec<bool>>, id: i32) -> HtmlResponder {
  let inner = checks.into_inner();
  match get_session(id) {
    Err(error) => match error {
      DIWKError::NotFound => Err(Custom(Status::NotFound, Html(SESSION_FAIL))),
      DIWKError::DieselError(_) => Err(Custom(Status::InternalServerError, Html("<p>Internal server error</p>"))) 
    },
    Ok(result) => {
      match integerify(inner, result.max_checks as usize) {
        None => Err(Custom(Status::BadRequest, Html("<p>filler</p>"))),
        Some(integer) => {
          let connection = start_connection();
          if result.opinion.signum() == -1 {
            match diesel::delete(opinionsessions::table.filter(opinionsessions::columns::id.eq(result.id))).execute(&connection) {
              Err(_) => Err(Custom(Status::InternalServerError, Html("<p>No Good</p>"))),
              Ok(_) => {
                println!("{}", (result.opinion & integer) ^ std::i64::MIN);
                Ok(Html("<p>Guni gugu</p>"))
                
              }
            }
          } else {
            match diesel::update(opinionsessions::table.filter(opinionsessions::columns::id.eq(result.id))).set(opinionsessions::columns::opinion.eq(integer)).get_result::<OpinionSessionQuery>(&connection) {
              Ok(_) => Ok(Html("<p>Ok</p>")),
              Err(_) => Err(Custom(Status::InternalServerError, Html("<p>No Good</p>")))
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
        Ok(x) => Ok(redirect(format!("/play/{}", x.id))),
        Err(_) => Err(Custom(Status::InternalServerError, Html(INTERNAL_SESSION_ERROR)))
      } 

    },
    Err(_) => Err(Custom(Status::UnprocessableEntity, Html(SESSION_FAIL)))
    /*
    Ok(x) => {
      match x.get(0) {
        Some(_) => {
          let connection = start_connection();
          match diesel::insert(&value).into(opinionsessions::table).get_result::<OpinionSessionQuery>(&connection) {
            Ok(x) => Ok(redirect(format!("/play/{}", x.id))),
            Err(_) => Err(Custom(Status::InternalServerError, Html(INTERNAL_SESSION_ERROR)))
          } 
        },
        None => Err(Custom(Status::new(422, "Invalid Chart ID"), Html(SESSION_FAIL)))
      }
    },
  Err(_) => Err(Custom(Status::InternalServerError, Html(INTERNAL_SESSION_ERROR)))
  */
  } 
}


#[post("/create", format="application/json", data="<upload>")]
fn post_create(upload: Json<OpinionChartJSON>) -> RedirectResponder {
  let form = upload.into_inner();
  let connection = start_connection();
  println!("{:?}", form);
  match diesel::insert(&form).into(opinioncharts::table).get_result::<OpinionChartSQL>(&connection) {
    Ok(_) => Ok(redirect(String::from("/session"))),
    Err(_) => Err(Custom(Status::InternalServerError, Html(INTERNAL_SESSION_ERROR))) 
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

#[derive(Queryable, Serialize, AsChangeset)]
#[table_name="opinionsessions"]
struct OpinionSessionQuery {
  id: i32,
  chart_id: i32,
  max_checks: i16,
  opinion: i64,
}

fn main() {
    dotenv().ok();
    println!("{}", integerify(vec![true, false, true, true], 4).unwrap());
    rocket::ignite()
    .mount("/", routes![home, started, create, post_create, start_game, actually_start_game, answer])
    .attach(Template::fairing())
    .launch();
}
