<script lang="ts">
	import { onMount } from 'svelte';
	import { goto, afterNavigate } from '$app/navigation';
	import { page } from '$app/stores';
	import { CSS } from '$lib/styles.js';

	let { children } = $props();

	// Show splash only on first visit per session
	const isFirstVisit = typeof sessionStorage !== 'undefined'
		? !sessionStorage.getItem('autopipe_visited')
		: true;

	let splashVisible = $state(isFirstVisit);
	let splashFading = $state(false);
	let appReady = $state(!isFirstVisit);

	afterNavigate(() => {
		document.documentElement.scrollTop = 0;
		document.body.scrollTop = 0;
	});

	onMount(() => {
		if (!isFirstVisit) {
			// Already visited: no splash, stay on current page
			return;
		}

		// First visit: show splash then reveal content
		sessionStorage.setItem('autopipe_visited', '1');

		setTimeout(() => {
			splashFading = true;
			setTimeout(() => {
				splashVisible = false;
				appReady = true;
			}, 500);
		}, 1200);
	});
</script>

<svelte:head>
	{@html `<style>${CSS}</style>`}
</svelte:head>

{#if splashVisible}
	<div class="splash" class:splash-fade={splashFading}>
		<div class="splash-inner">
			<div class="splash-icon">
				<span class="dot"></span><span class="line"></span><span class="dot"></span><span
					class="line"
				></span><span class="dot"></span>
			</div>
			<div class="splash-title">Autopipe Hub</div>
			<div class="splash-sub"></div>
			<div class="splash-bar"><div class="splash-bar-fill"></div></div>
			<div class="splash-loading">Loading...</div>
		</div>
	</div>
{/if}

<div class:app-hidden={!appReady}>
	<header>
		<div class="header-top">
			<a href="/" class="logo"><img src="/logo.png" alt="" class="logo-icon">Autopipe Hub</a>
		</div>
	</header>
	{@render children()}
</div>
