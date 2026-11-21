import { redirect } from '@sveltejs/kit';

// Paper sessions are robots now: the old monitor link opens the robot's page.
export function load({ params }) {
  redirect(307, `/robots/${params.id}`);
}
