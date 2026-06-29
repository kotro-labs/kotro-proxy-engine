import http from 'k6/http';
import { check, sleep } from 'k6';

export const options = {
  vus: 30,
  duration: '20s',
  thresholds: {
    http_req_failed: ['rate<0.05'],
    http_req_duration: ['p(95)<2000'],
  },
};

export default function () {
  const payload = JSON.stringify({
    model: 'gpt-4',
    stream: true,
    messages: [
      { role: 'system', content: 'bench' },
      { role: 'user', content: `unique-${__VU}-${__ITER}-${Date.now()}` },
    ],
  });

  const res = http.post('http://127.0.0.1:8080/v1/chat/completions', payload, {
    headers: { 'Content-Type': 'application/json' },
    tags: { scenario: 'cache_miss' },
  });
  check(res, {
    'status 200': (r) => r.status === 200,
    'cache miss': (r) => !r.headers['X-Kortolabs-Cache'],
  });
  sleep(0.05);
}
