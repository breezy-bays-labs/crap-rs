type Event = { preventDefault?: () => void };

export class Form {
  onClick = () => {
    this.touched = true;
  };

  onSubmit = (e: Event) => {
    if (e.preventDefault) e.preventDefault();
  };

  static setup = function () {
    return new Form();
  };

  touched = false;
}
