export function csrf(node) {
  const token = document.body.dataset.csrf;
  if (!token) return;

  const input = document.createElement('input');
  input.type = 'hidden';
  input.name = 'csrf_token';
  input.value = token;

  node.appendChild(input);
}
