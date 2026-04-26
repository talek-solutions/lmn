// Test 3 — POST with generated body, 5_000 requests at 20 VUs
// Mirrors comparison/lmn/3_request.json: same field names, same value domains.

import http from 'k6/http';

export const options = {
  scenarios: {
    fixed_iterations: {
      executor: 'shared-iterations',
      vus: 20,
      iterations: 5000,
      maxDuration: '5m',
    },
  },
  thresholds: {
    http_req_failed: ['rate<0.01'],
    http_req_duration: ['p(95)<2000'],
    http_reqs: ['rate>=10'],
  },
  summaryTrendStats: ['avg', 'min', 'med', 'max', 'p(90)', 'p(95)', 'p(99)'],
};

const URL = 'http://localhost:3000/load-test/process';
const params = { headers: { 'Content-Type': 'application/json' } };

const FIELD_STRINGS = ['alpha', 'beta', 'gamma', 'delta', 'epsilon'];
const LOWERCASE = 'abcdefghijklmnopqrstuvwxyz';

function pick(arr) {
  return arr[Math.floor(Math.random() * arr.length)];
}

function intBetween(min, max) {
  return Math.floor(Math.random() * (max - min + 1)) + min;
}

function floatBetween(min, max, decimals) {
  const v = Math.random() * (max - min) + min;
  return Number(v.toFixed(decimals));
}

function lowercaseString(min, max) {
  const len = intBetween(min, max);
  let s = '';
  for (let i = 0; i < len; i++) s += LOWERCASE[intBetween(0, 25)];
  return s;
}

function buildBody() {
  return JSON.stringify({
    fieldString: pick(FIELD_STRINGS),
    fieldInteger: intBetween(1, 1000),
    objectField: {
      nestedField1: lowercaseString(5, 15),
      nestedField2: floatBetween(0, 100, 1),
    },
  });
}

export default function () {
  http.post(URL, buildBody(), params);
}
