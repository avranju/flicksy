use topcoat::{
    Result,
    router::page,
    view::{View, view},
};

#[page("/terms")]
async fn terms() -> Result<impl View> {
    Ok(view! {
        <main class="mx-auto max-w-[824px] px-6 pt-10 pb-16">
            <h1 class="text-3xl font-bold">"Terms of service"</h1>
            <div class="mt-8 flex flex-col gap-6 rounded-[20px] bg-card p-6 leading-relaxed sm:p-10">
                <p>"These terms apply to your use of this installation of Flicksy. Flicksy helps you create videos from music, images, and instructions, and may offer an option to upload them to YouTube."</p>
                <section class="flex flex-col gap-3">
                    <h2 class="text-xl font-bold">"Your account and content"</h2>
                    <p>"Keep your account credentials secure and use Flicksy only with an account you are authorized to access. You are responsible for the content you provide and publish. Only upload or use music, images, text, and other materials that you have the rights or permission to use."</p>
                </section>
                <section class="flex flex-col gap-3">
                    <h2 class="text-xl font-bold">"Video generation"</h2>
                    <p>"Flicksy uses automated tools and a configured language-model service to produce videos. Review generated videos before sharing them. The availability and results of those tools can vary."</p>
                </section>
                <section class="flex flex-col gap-3">
                    <h2 class="text-xl font-bold">"YouTube"</h2>
                    <p>"Flicksy uses YouTube API Services when you connect a channel or upload a video. By using Flicksy, you agree to be bound by the " <a href="https://www.youtube.com/t/terms" target="_blank" rel="noopener noreferrer" class="text-accent-ink underline">"YouTube Terms of Service"</a> ". You are responsible for reviewing the video and its details, choosing the intended visibility, and complying with YouTube's applicable policies when uploading."</p>
                    <p>"You can disconnect YouTube from Flicksy's account settings and manage or revoke access through your " <a href="https://security.google.com/settings/security/permissions" target="_blank" rel="noopener noreferrer" class="text-accent-ink underline">"Google account permissions"</a> "."</p>
                </section>
                <section class="flex flex-col gap-3">
                    <h2 class="text-xl font-bold">"Privacy and contact"</h2>
                    <p>"Our " <a href="/privacy" class="text-accent-ink underline">"privacy policy"</a> " explains how Flicksy handles information. For questions about these terms, contact " <a href="mailto:avranju@nerdworks.dev" class="text-accent-ink underline">"avranju@nerdworks.dev"</a> "."</p>
                </section>
            </div>
        </main>
    })
}
