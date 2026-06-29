import http from 'k6/http';
import { check, sleep } from 'k6';

export const options = {
  vus: 50,
  duration: '20s',
  thresholds: {
    http_req_failed: ['rate<0.01'],
    http_req_duration: ['p(95)<500'],
  },
};

const payload = JSON.stringify({
  model: 'gpt-4',
  stream: true,
  messages: [
    { role: 'system', content: 'bench' },
    { role: 'user', content: 'warm-openai' },
  ],
});

export default function () {
  const res = http.post('http://127.0.0.1:8080/v1/chat/completions', payload, {
    headers: { 'Content-Type': 'application/json' },
    tags: { scenario: 'cache_hit' },
  });
  check(res, {
    'status 200': (r) => r.status === 200,
    'cache hit': (r) => r.headers['X-Kortolabs-Cache'] === 'HIT',
  });
  sleep(0.01);
}
