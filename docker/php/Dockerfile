# The image `cargo xtask php-stubs` dumps from: an official PHP image plus extensions —
# the ones PHP ships but doesn't compile in by default (intl, gd, zip…) and, for the
# newest versions, popular PECL ones (redis, imagick, http…) — so the dump sees them.
# Extensions are installed with mlocati/docker-php-extension-installer (MIT); only
# developers / CI build this image, nothing of it ships with Lumen.
ARG PHP=8.4
FROM php:${PHP}-cli-alpine
COPY --from=mlocati/php-extension-installer:2 /usr/bin/install-php-extensions /usr/local/bin/
ARG EXTENSIONS=""
# One at a time: an extension that doesn't build for this PHP version is skipped.
RUN for e in $EXTENSIONS; do install-php-extensions "$e" || echo "lumen: skipped $e"; done
