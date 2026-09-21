ARG OPENCLAW_BASE_IMAGE=ghcr.io/openclaw/openclaw:2026.7.1-2
FROM ${OPENCLAW_BASE_IMAGE}

USER root

COPY bin/support-telegram-notify.mjs /usr/local/bin/support-telegram-notify
COPY bin/support-resolve-conversation.mjs /usr/local/bin/support-resolve-conversation
COPY bin/support-schedule-reminder.mjs /usr/local/bin/support-schedule-reminder
COPY bin/support-public-http.mjs /usr/local/bin/support-public-http
COPY bin/support-public-http.mjs /usr/local/bin/support-public-http.mjs
COPY bin/support-public-network.mjs /usr/local/bin/support-public-network.mjs
COPY bin/support-browser.mjs /usr/local/bin/support-browser
COPY bin/support-browser-proxy.mjs /usr/local/bin/support-browser-proxy.mjs
COPY bin/support-browser-protocol.mjs /usr/local/bin/support-browser-protocol.mjs
COPY bin/support-curl.mjs /usr/local/bin/curl
COPY bin/support-knowledge-article.mjs /usr/local/bin/support-knowledge-article
COPY bin/support-test-tool.mjs /usr/local/bin/support-test-tool.mjs
COPY bin/support-integration.mjs /usr/local/bin/support-integration.mjs
COPY bin/support-integration.mjs /usr/local/bin/support-integration
COPY bin/support-shell.mjs /usr/local/bin/support-shell
COPY bin/support-shell-runner.mjs /usr/local/lib/support-shell-runner.mjs
RUN chmod 0444 /usr/local/bin/support-test-tool.mjs /usr/local/bin/support-integration.mjs \
    && test -x /usr/bin/curl \
    && chown root:root /usr/local/bin/curl /usr/local/bin/support-public-http.mjs \
    && chmod 0555 /usr/local/bin/curl \
    && chmod 0444 /usr/local/bin/support-public-http.mjs /usr/local/bin/support-public-network.mjs /usr/local/bin/support-browser-protocol.mjs \
    && chown root:root \
        /usr/local/bin/support-telegram-notify \
        /usr/local/bin/support-resolve-conversation \
        /usr/local/bin/support-schedule-reminder \
        /usr/local/bin/support-public-http \
        /usr/local/bin/support-browser \
        /usr/local/bin/support-knowledge-article \
        /usr/local/bin/support-integration \
        /usr/local/bin/support-shell \
        /usr/local/lib/support-shell-runner.mjs \
    && chmod 0555 \
        /usr/local/bin/support-telegram-notify \
        /usr/local/bin/support-resolve-conversation \
        /usr/local/bin/support-schedule-reminder \
        /usr/local/bin/support-public-http \
        /usr/local/bin/support-browser \
        /usr/local/bin/support-knowledge-article \
        /usr/local/bin/support-integration \
        /usr/local/bin/support-shell \
        /usr/local/lib/support-shell-runner.mjs

USER node
