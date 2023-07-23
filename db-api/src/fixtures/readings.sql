insert into readings(sensor_id, timestamp, type, reading)
values (1, to_timestamp(1690127100), '{"minimum", "maximum"}', '{0.1, 10.2}'),
			 (2, to_timestamp(1690127200), '{"minimum", "maximum", "average", "median", "count"}', '{0.5, 100.2, 50.8, 48.2, 1002.0}'),
			 (3, to_timestamp(1690127300), '{"average", "maximum"}', '{20.1, 100.2}'),
			 (4, to_timestamp(1690127400), '{"median", "count"}', '{15.1, 10.0}'),
			 (4, to_timestamp(1690127410), '{"median", "count"}', '{25.2, 20.0}'),
			 (4, to_timestamp(1690127420), '{"median", "count"}', '{35.3, 30.0}');
