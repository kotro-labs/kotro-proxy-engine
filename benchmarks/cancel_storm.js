import http from 'k6/http';
import { check, sleep } from 'k6';

const proxyURL = __ENV.KORTO_PROXY_URL || 'http://127.0.0.1:8080';
// Use AUDIT_* names — K6_VUS/K6_DURATION are reserved and override scenario config.
const vus = Number(__ENV.AUDIT_VUS || __ENV.K6_VUS || 100);
const duration = __ENV.AUDIT_DURATION || __ENV.K6_DURATION || '30s';

export const options = {
  scenarios: {
    cancel_storm: {
      executor: 'constant-vus',
      vus,
      duration,
    },
  },
};

export default function () {
  const payload = JSON.stringify({
    model: 'gpt-4',
    stream: true,
    messages: [
      {
        role: 'user',
        content: `cancel-storm-${__VU}-${__ITER}-${Date.now()}`,
      },
    ],
  });

  const params = {
    headers: { 'Content-Type': 'application/json' },
    timeout: __ENV.K6_TIMEOUT || '500ms',
    tags: { scenario: 'cancel_storm' },
  };

  const res = http.post(`${proxyURL}/v1/chat/completions`, payload, params);
  check(res, {
    'request issued': (r) => r.status === 0 || r.status === 200 || r.status === 499,
  });

  sleep(0.05);
}
