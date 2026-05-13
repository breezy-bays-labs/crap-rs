import { useCallback } from 'react';
export function Button({ onClick }: { onClick: () => void }) {
  const handle = useCallback(() => { onClick(); }, [onClick]);
  return <button onClick={handle}>Click</button>;
}
