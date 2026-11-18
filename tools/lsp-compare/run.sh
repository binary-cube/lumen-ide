#!/usr/bin/env bash
# The PHP engines' comparison (plans 43, 47 and 65d): the built-in engine, intelephense and
# phpactor asked the same 121 probes on the same projects, scored side by side.
#
#   tools/lsp-compare/run.sh <work dir> [ours|intelephense|phpactor|score]…
#
# Everything is made in <work dir> (a scratch folder; never the app's data folder): the
# projects (`features` copied from projects/features; a Laravel and a Symfony skeleton made
# with Composer, the probe files copied in), the servers (intelephense from npm, phpactor's
# release PHAR checked against the SHA-256 the app pins), their caches and a HOME of their
# own. The built-in engine answers through the app itself (an ignored test of the app,
# `the_comparisons_probes_answered_by_the_built_in_engine`, run with that HOME too).
# Needs Composer, PHP (phpactor's), Node and npm (intelephense's); the results go to
# <work dir>/score.md and details.md. No server is left running.

set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
WORK="${1:?the work dir}"
shift
mkdir -p "$WORK"
WORK="$(cd "$WORK" && pwd -P)"
STEPS=("$@")
[ "${#STEPS[@]}" -eq 0 ] && STEPS=(ours intelephense phpactor score)

INTELEPHENSE_VERSION="1.18.5"
PHPACTOR_VERSION="2026.10.07.0"
PHPACTOR_SHA256="b28bf6166e016d539fc2f2aec18ef918b12590e436a6f1ccbdf30b5bbdbdb16b"

export COMPOSER_HOME="$WORK/home/composer"
mkdir -p "$WORK/projects" "$WORK/servers" "$WORK/home"

# The projects.
if [ ! -d "$WORK/projects/laravel/vendor" ]; then
    composer create-project --no-interaction --prefer-dist laravel/laravel "$WORK/projects/laravel"
fi
if [ ! -d "$WORK/projects/symfony/vendor" ]; then
    composer create-project --no-interaction --prefer-dist symfony/skeleton "$WORK/projects/symfony"
fi
rm -rf "$WORK/projects/features"
cp -R "$HERE/projects/features" "$WORK/projects/features"
(cd "$WORK/projects/features" && composer dump-autoload --quiet)
cp "$HERE/projects/laravel/app/LaravelProbe.php" "$WORK/projects/laravel/app/"
cp "$HERE/projects/symfony/src/Controller/ProbeController.php" \
    "$HERE/projects/symfony/src/Controller/SymfonyProblems.php" "$WORK/projects/symfony/src/Controller/"
mkdir -p "$WORK/projects/symfony/translations"
cp "$HERE/projects/symfony/translations/messages.en.yaml" "$WORK/projects/symfony/translations/"
# The container compiled (`var/cache/dev/*Container.xml`): every service and parameter the
# Symfony module knows (plan 65d). Needs the translator (`t()`) and the router.
if [ ! -d "$WORK/projects/symfony/vendor/symfony/translation" ]; then
    (cd "$WORK/projects/symfony" && composer require --no-interaction symfony/translation)
fi
(cd "$WORK/projects/symfony" && php bin/console cache:warmup --env=dev --quiet)
python3 -I "$HERE/probes.py" "$WORK"

# The servers.
if [ ! -f "$WORK/servers/intelephense/node_modules/intelephense/lib/intelephense.js" ]; then
    npm_config_cache="$WORK/home/npm" npm install --prefix "$WORK/servers/intelephense" \
        --no-audit --no-fund "intelephense@$INTELEPHENSE_VERSION"
fi
if [ ! -f "$WORK/servers/phpactor/phpactor.phar" ]; then
    mkdir -p "$WORK/servers/phpactor"
    curl -fsSL -o "$WORK/servers/phpactor/phpactor.phar.part" \
        "https://github.com/phpactor/phpactor/releases/download/$PHPACTOR_VERSION/phpactor.phar"
    echo "$PHPACTOR_SHA256  $WORK/servers/phpactor/phpactor.phar.part" | shasum -a 256 -c -
    mv "$WORK/servers/phpactor/phpactor.phar.part" "$WORK/servers/phpactor/phpactor.phar"
fi

for step in "${STEPS[@]}"; do
    case "$step" in
        ours)
            (cd "$ROOT" && env HOME="$WORK/home" CARGO_HOME="${CARGO_HOME:-$HOME/.cargo}" \
                RUSTUP_HOME="${RUSTUP_HOME:-$HOME/.rustup}" \
                EDITOR_PROBES="$WORK/probes.json" EDITOR_PROBE_ANSWERS="$WORK/answers-ours.json" \
                cargo test -p app --release the_comparisons_probes -- --ignored --nocapture)
            ;;
        intelephense | phpactor)
            python3 -I "$HERE/client.py" "$WORK" "$step"
            ;;
        score)
            python3 -I "$HERE/score.py" "$WORK"
            ;;
        *)
            echo "unknown step: $step" >&2
            exit 1
            ;;
    esac
done
