import { mount } from 'svelte';

import App from './App.svelte';
import './lib/styles/tokens.css';
import './lib/styles/themes.css';
import './lib/styles/global.css';

const target = document.getElementById('app');

if (!target) {
  throw new Error('Readloom app root was not found.');
}

mount(App, { target });

