const baseUrl = process.env.EPICODE_BASE_URL ?? "http://localhost:8080/api";
const apiKey = process.env.EPICODE_API_KEY;

if (!apiKey) {
  throw new Error("Set EPICODE_API_KEY before running this example.");
}

async function api(path, body) {
  const response = await fetch(`${baseUrl}${path}`, {
    method: body ? "POST" : "GET",
    headers: {
      "Content-Type": "application/json",
      "X-API-Key": apiKey,
    },
    body: body ? JSON.stringify(body) : undefined,
  });

  const data = await response.json();
  if (!response.ok) {
    throw new Error(JSON.stringify(data));
  }
  return data;
}

const remembered = await api("/v1/remember", {
  content: "Node example stored a deployment memory",
});
console.log("remember:", remembered);

const search = await api("/v1/search", {
  query: "deployment memory",
  limit: 3,
});
console.log("search:", search);

const answer = await api("/v1/ask", {
  question: "What memory did the Node example store?",
  depth: 2,
});
console.log("ask:", answer);
