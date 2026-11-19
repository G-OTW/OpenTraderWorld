// Lets Node tests import modules that use SvelteKit's `$lib/` alias.
import { register } from 'node:module';

register('./lib-alias-hooks.mjs', import.meta.url);
