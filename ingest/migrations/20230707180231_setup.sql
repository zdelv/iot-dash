CREATE TABLE sensors (
	sensor_id SERIAL NOT NULL UNIQUE,
	topic varchar(255) NOT NULL UNIQUE,
	PRIMARY KEY(sensor_id)
);

CREATE TYPE READING_TYPE AS ENUM ('minimum', 'maximum', 'average', 'median', 'count');
CREATE TABLE stats (
	reading_id SERIAL NOT NULL UNIQUE,
	sensor_id INT NOT NULL,
	timestamp TIMESTAMPTZ NOT NULL,
	type READING_TYPE[] NOT NULL,
	reading REAL[] NOT NULL,
	PRIMARY KEY(reading_id),
	CONSTRAINT fk_sensor
		FOREIGN KEY(sensor_id)
			REFERENCES sensors(sensor_id)
);
ALTER TABLE stats ALTER COLUMN timestamp SET DEFAULT NOW();
