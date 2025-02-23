require('dotenv').config({ path: __dirname + `/../${process.env.ENV_PATH}` });
const Sentry = require("@sentry/node");

Sentry.init({
    dsn: process.env.GMP_API_SENTRY_DSN,
    integrations: [],
    // Tracing
    tracesSampleRate: 1.0, //  Capture 100% of the transactions
});