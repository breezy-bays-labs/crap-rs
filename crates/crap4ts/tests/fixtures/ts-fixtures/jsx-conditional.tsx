type Props = { visible: boolean; name: string };

export function Greeting({ visible, name }: Props) {
  return <div>{visible && <span>hello, {name}</span>}</div>;
}
