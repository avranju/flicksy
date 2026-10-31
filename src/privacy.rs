use topcoat::{
    Result,
    router::page,
    view::{View, view},
};

#[page("/privacy")]
async fn privacy() -> Result<impl View> {
    Ok(view! {
        <main class="mx-auto max-w-[824px] px-6 pt-10 pb-16">
            <h1 class="text-3xl font-bold">"Privacy policy"</h1>
            <div class="mt-8 flex flex-col gap-6 rounded-[20px] bg-card p-6 leading-relaxed sm:p-10">
                <p>"Flicksy helps users create videos from music, images, and instructions. This policy describes how Flicksy handles information when you use this installation of the service."</p>
                <section class="flex flex-col gap-3">
                    <h2 class="text-xl font-bold">"Information Flicksy stores"</h2>
                    <p>"Flicksy stores your account username, a salted password hash, and sign-in session records. It uses a session cookie to keep you signed in. Flicksy also stores the project titles, instructions, credits, media files, and rendered videos you provide or create."</p>
                    <p>"The server and its hosting infrastructure may record operational information, such as request timing and error logs, to run and troubleshoot the service."</p>
                </section>
                <section class="flex flex-col gap-3">
                    <h2 class="text-xl font-bold">"Video production"</h2>
                    <p>"To create a video, Flicksy sends project information, including titles, credits, instructions, and asset metadata, to the language-model service configured by the operator. Tool results from the production process may also be sent to that service. The service may be hosted by a third party; its handling of this information depends on the operator's configuration and the provider's terms."</p>
                </section>
                <section class="flex flex-col gap-3">
                    <h2 class="text-xl font-bold">"YouTube and Google data"</h2>
                    <p>"If you choose to connect YouTube, Google asks for your permission to let Flicksy identify your YouTube channel and upload videos. Flicksy stores the connected channel's ID and title, an encrypted authorization token, and upload records. Upload records include the video details you supply, transfer status, and any YouTube video ID returned. An encrypted temporary upload-session URL may be stored to resume an interrupted upload."</p>
                    <p>"Flicksy sends the video and the title, description, visibility, and audience settings you select to YouTube when you initiate an upload. Google and YouTube handle that information under their own terms and policies."</p>
                    <p>
                        "You can disconnect your channel in Flicksy's YouTube settings, which removes Flicksy's stored connection. You can also revoke Flicksy's access in your "
                        <a class="text-accent-ink underline" href="https://security.google.com/settings/security/permissions" target="_blank" rel="noopener noreferrer">"Google account permissions"</a>
                        ". Disconnecting does not delete videos already uploaded to YouTube. See the "
                        <a class="text-accent-ink underline" href="https://policies.google.com/privacy" target="_blank" rel="noopener noreferrer">"Google Privacy Policy"</a>
                        " and "
                        <a class="text-accent-ink underline" href="https://www.youtube.com/t/terms" target="_blank" rel="noopener noreferrer">"YouTube Terms of Service"</a>
                        "."
                    </p>
                </section>
                <section class="flex flex-col gap-3">
                    <h2 class="text-xl font-bold">"Retention and deletion"</h2>
                    <p>"Flicksy keeps account, session, project, and upload information so you can use the service and return to your work. Signing out removes the current session. Disconnecting YouTube removes the stored connection; upload history and videos already on YouTube are not automatically deleted."</p>
                    <p>"To request access to or deletion of your Flicksy account, projects, or stored YouTube information, contact the operator at " <a class="text-accent-ink underline" href="mailto:avranju@nerdworks.dev">"avranju@nerdworks.dev"</a> ". Copies may also remain in backups until those backups are replaced or removed. Manage videos uploaded to YouTube through YouTube Studio."</p>
                </section>
                <section class="flex flex-col gap-3">
                    <h2 class="text-xl font-bold">"Contact"</h2>
                    <p>"For privacy questions or requests, email " <a class="text-accent-ink underline" href="mailto:avranju@nerdworks.dev">"avranju@nerdworks.dev"</a> "."</p>
                </section>
            </div>
        </main>
    })
}
