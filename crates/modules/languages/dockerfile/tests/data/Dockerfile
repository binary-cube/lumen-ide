# syntax=docker/dockerfile:1
ARG PHP_VERSION=8.4

# Build the assets.
FROM node:22-alpine AS assets
WORKDIR /app
COPY package.json package-lock.json ./
RUN --mount=type=cache,target=/root/.npm npm ci \
    && npm run build

FROM php:${PHP_VERSION}-fpm@sha256:0123456789abcdef AS app
ENV APP_ENV=production \
    APP_DEBUG=0
LABEL org.opencontainers.image.title="shop"
COPY --from=assets /app/public/build /var/www/public/build
RUN <<EOT
apt-get update
apt-get install -y libzip-dev
EOT
EXPOSE 9000
USER www-data
HEALTHCHECK --interval=30s CMD php-fpm -t || exit 1
CMD ["php-fpm", "--nodaemonize"]
