import http from 'k6/http';
import { check, sleep } from 'k6';

export const options = {
  vus: 60,
  duration: '25s',
  thresholds: {
    http_req_failed: ['rate<0.02'],
  },
};

export default function () {
  const isHit = __ITER % 3 !== 0;
  const openai = __VU % 2 === 0;

  if (openai) {
    const user = isHit ? 'warm-openai' : `mixed-openai-${__VU}-${__ITER}`;
    const payload = JSON.stringify({
      model: 'gpt-4',
      stream: true,
      messages: [
        { role: 'system', content: 'bench' },
        { role: 'user', content: user },
      ],
    });
    const res = http.post('http://127.0.0.1:8080/v1/chat/completions', payload, {
      headers: { 'Content-Type': 'application/json' },
    });
    check(res, { 'openai 200': (r) => r.status === 200 });
  } else {
    const user = isHit ? 'warm-anthropic' : `mixed-anthropic-${__VU}-${__ITER}`;
    const payload = JSON.stringify({
      model: 'claude-3-5-sonnet-20241022',
      max_tokens: 64,
      stream: true,
      system: 'bench',
      messages: [{ role: 'user', content: user }],
    });
    const res = http.post('http://127.0.0.1:8080/v1/messages', payload, {
      headers: {
        'Content-Type': 'application/json',
        'x-api-key': 'bench-key',
      },
    });
    check(res, { 'anthropic 200': (r) => r.status === 200 });
  }
  sleep(0.02);
}
