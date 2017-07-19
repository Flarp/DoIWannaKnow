-- Your SQL goes here
CREATE TABLE OPINIONCHARTS (
  id SERIAL PRIMARY KEY NOT NULL,
  title TEXT NOT NULL,
  description TEXT NOT NULL,
  opinions TEXT[] NOT NULL
);

CREATE TABLE OPINIONSESSIONS (
  id SERIAL PRIMARY KEY NOT NULL,
  chart_id INT REFERENCES OPINIONCHARTS(id) NOT NULL,
  max_checks SMALLINT NOT NULL,
  opinion BIGINT NOT NULL DEFAULT 0,
  done BOOLEAN NOT NULL DEFAULT 'f'
);


