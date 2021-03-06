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
  read_pass INT NOT NULL DEFAULT 0,
  write_pass INT NOT NULL DEFAULT 0,
  creation_time BIGINT NOT NULL DEFAULT EXTRACT(epoch FROM now()) * 1000
);


