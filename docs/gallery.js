(() => {
  const feature = document.getElementById('feature-view');
  const theme = document.getElementById('capture-theme');
  const image = document.getElementById('capture-image');
  const link = document.getElementById('capture-link');
  const caption = document.getElementById('capture-caption');
  const views = {
    table: ['Browse and inspect.', 'Object navigation, column types, filters, sorting, pagination, row controls and the pinned activity inspector.'],
    'sql-editor': ['Work in SQL.', 'Run and run-selection controls, EXPLAIN, snippets, saved queries and a pinned inspector. The light capture shows a sample query; the dark capture shows a new query tab.'],
    relationships: ['Follow the relationships.', 'Customers and orders, primary and foreign keys, diagram navigation and SVG export controls.'],
    palette: ['Reach for a command.', 'Search commands and database objects, open query tabs, export data and jump to preferences from the keyboard.']
  };
  let featureValue = 'table', themeValue = 'auto';
  const media = matchMedia('(prefers-color-scheme: dark)');
  function render() {
    const selected = document.documentElement.dataset.theme || (media.matches ? 'dark' : 'light');
    const mode = themeValue === 'auto' ? selected : themeValue;
    const [title, description] = views[featureValue];
    image.src = `screenshots/${featureValue}-${mode}.jpg`;
    link.href = image.src;
    image.alt = `Actual dboard desktop app in ${mode} mode. ${title} ${description}`;
    caption.replaceChildren();
    const strong = document.createElement('strong');
    strong.textContent = title;
    caption.append(strong, ` ${description} ${mode === 'dark' ? 'Dark' : 'Light'} theme. Open the image for full resolution.`);
  }
  function bind(group, update) {
    group.addEventListener('click', event => {
      const button = event.target.closest('button');
      if (!button) return;
      update(button.dataset.value);
      group.querySelectorAll('button').forEach(item => item.setAttribute('aria-pressed', String(item === button)));
      render();
    });
  }
  bind(feature, value => featureValue = value);
  bind(theme, value => themeValue = value);
  media.addEventListener('change', render);
  new MutationObserver(render).observe(document.documentElement, {attributes:true, attributeFilter:['data-theme']});
  render();
})();
