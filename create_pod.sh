#!/bin/bash

POD_NAME=$1
NETWORK_NAME=${POD_NAME}_network
VOLUME_NAME=${POD_NAME}_volume

if [[ $POD_NAME == "" ]]; then
	echo "Please supply a name for the pod."
	exit
fi


# Create the pod, network, and volume if they do not already exist.
podman pod inspect $POD_NAME || podman pod create $POD_NAME
podman network inspect $NETWORK_NAME || podman network create $NETWORK_NAME
podman volume inspect $VOLUME_NAME || podman volume create $VOLUME_NAME

# MQTT
podman create \
	--pod $POD_NAME \
	--net $NETWORK_NAME \
	--network-alias mqtt \
	--name mqtt \
	-p 1883:1883 \
	--volume ./configs/mosquitto.conf:/mosquitto/config/mosquitto.conf \
	docker.io/eclipse-mosquitto

# PostgreSQL
podman create \
	--pod $POD_NAME \
	--net $NETWORK_NAME \
	--network-alias db \
	--name db \
	-p 5432:5432 \
	-e POSTGRES_PASSWORD_FILE=/run/secrets/db_password \
	--volume ./configs/db_password.txt:/run/secrets/db_password \
	--volume ./configs/db:/docker-entrypoint-initdb.d \
	docker.io/postgres

# Adminer
podman create \
	--pod $POD_NAME \
	--net $NETWORK_NAME \
	--network-alias adminer \
	--name adminer \
	-p 8081:8080 \
	docker.io/adminer

# If the ingest image isn't already created, then create it.
if [[ $(podman inspect ingest) == "[]" ]]; then
	echo "Could not find the ingest image. Building it now."
	cd ingest
	podman build -t ingest ../ --file Dockerfile
	cd ..
fi

# Ingest
podman create \
	--pod $POD_NAME \
	--net $NETWORK_NAME \
	--network-alias ingest \
	--name ingest \
	--restart on-failure \
	-e CONFIG=config.yaml \
	-e POD=true \
	-e LOG_DIR=/logs \
	--volume ./ingest/.env:/.env \
	--volume ./ingest/config.yaml:/config.yaml \
	--volume ./pod_logs:/logs \
	ingest \
	ingest

# If the db-api image isn't already created, then create it.
if [[ $(podman inspect db-api) == "[]" ]]; then
	echo "Could not find the db-api image. Building it now."
	cd db-api
	podman build -t db-api ../ --file Dockerfile
	cd ..
fi

# db-api
podman create \
	--pod $POD_NAME \
	--net $NETWORK_NAME \
	--network-alias db-api \
	--name db-api \
	--restart on-failure \
	-p 3001:3001 \
	-e POD=true \
	-e LOG_DIR=/logs \
	--volume ./db-api/.env:/.env \
	--volume ./pod_logs:/logs \
	db-api \
	db-api

if [[ ! -d pod_logs ]]; then
	mkdir pod_logs
fi
