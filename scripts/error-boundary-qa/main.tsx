// Exercises the production React boundary by throwing during child render.
// Loads the same design tokens and components as the production application.
import React from 'react';
import { createRoot } from 'react-dom/client';
import '../../src/design/index.css';
import { FrontendErrorBoundary } from '../../src/components/FrontendErrorBoundary';

const params = new URLSearchParams(location.search);
const theme = params.get('theme');
if (theme === 'light' || theme === 'dark') document.documentElement.dataset.theme = theme;
function ThrowDuringRender(): React.ReactNode {
  throw new Error('HANDOFF_QA_EXPECTED_RENDER_ERROR：真实 React 渲染异常');
}
createRoot(document.getElementById('root')!).render(
  <FrontendErrorBoundary><ThrowDuringRender /></FrontendErrorBoundary>,
);
