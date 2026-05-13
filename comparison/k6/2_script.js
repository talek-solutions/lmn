// Test 2 — Curve execution, GET, 30s ramp 0→20 / 1m hold @20 / 30s ramp 20→0

import http from 'k6/http';

export const options = {
  scenarios: {
    ramp: {
      executor: 'ramping-vus',
      startVUs: 0,
      stages: [
        { duration: '30s', target: 20 },
        { duration: '1m',  target: 20 },
        { duration: '30s', target: 0  },
      ],
      gracefulRampDown: '0s',
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
