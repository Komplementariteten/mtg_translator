# syntax=docker/dockerfile:1

FROM node:24-alpine AS frontend
WORKDIR /app/frontend
COPY frontend/package.json frontend/package-lock.json ./
RUN npm ci
COPY frontend/ ./
RUN npm run build

FROM rust:alpine AS backend
RUN apk add --no-cache build-base cmake perl ca-certificates
WORKDIR /app
COPY backend/ backend/
COPY --from=frontend /app/frontend/dist frontend/dist
RUN cargo build --release --manifest-path backend/Cargo.toml

# Static binary with embedded frontend; CA certificates are needed for the image download.
FROM scratch
COPY --from=backend /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/ca-certificates.crt
COPY --from=backend /app/backend/target/release/backend /backend
ENV BIND_ADDRESS=0.0.0.0:3001 \
    CARD_DATABASE=/data \
    IMAGE_CACHE=/data/images \
    SSL_CERT_FILE=/etc/ssl/certs/ca-certificates.crt
VOLUME /data
EXPOSE 3001
ENTRYPOINT ["/backend"]
