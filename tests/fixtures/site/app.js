const users = fetch('/api/v1/users', { method: 'GET', headers: { Authorization: `Bearer ${token}` } });
const admin = fetch(`/api/v1/admin/users/${id}`, { method: 'DELETE' });
const remote = axios.get('https://api.example.com/v1/items');
const socket = new WebSocket('wss://api.example.com/socket');
const query = gql`query GetUsers { users { id email } }`;
const chunk = import('/assets/chunk-abc123.js');
const config = { apiUrl: '/api/v2/config', graphql: '/graphql' };
// sourceMappingURL=app.js.map
