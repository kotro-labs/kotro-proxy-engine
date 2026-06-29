import http from 'k6/http';
import { check, sleep } from 'k6';

export const options = {
  vus: 40,
  duration: '20s',
  thresholds: {
    http_req_failed: ['rate<0.01'],
    http_req_duration: ['p(95)<600'],
  },
};

const payload = JSON.stringify({
  model: 'claude-3-5-sonnet-20241022',
  max_tokens: 64,
  stream: true,
  system: 'bench',
  messages: [{ role: 'user', content: 'warm-anthropic' }],
});

export default function () {
  const res = http.post('http://127.0.0.1:8080/v1/messages', payload, {
    headers: {
      'Content-Type': 'application/json',
      'x-api-key': 'bench-key',
      'anthropic-version': '2023-06-01',
    },
    tags: { scenario: 'anthropic_hit' },
  });
  check(res, {
    'status 200': (r) => r.status === 200,
    'cache hit': (r) => r.headers['X-Kortolabs-Cache'] === 'HIT',
  });
  sleep(0.01);
}
