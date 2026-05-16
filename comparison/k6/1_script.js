// Test 1 — Fixed execution, GET, 10_000 requests at 20 VUs
// Endpoint is intentionally flaky (~50% 500s), so http_req_failed threshold is relaxed.

import http from 'k6/http';

export const options = {
  scenarios: {
    fixed_iterations: {
      executor: 'shared-iterations',
      vus: 20,
      iterations: 10000,
      maxDuration: '5m',
    },
  },
  thresholds: {
    http_req_failed: ['rate<0.7'],
    http_req_duration: ['p(95)<2000'],
    http_reqs: ['rate>=10'],
  },
  summaryTrendStats: ['avg', 'min', 'med', 'max', 'p(90)', 'p(95)', 'p(99)'],
};

const URL = 'http://localhost:3000/load-test/random-error';

export default function () {
  http.get(URL);
}
