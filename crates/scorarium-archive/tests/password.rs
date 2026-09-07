use scorarium_archive::{Archive, PasswordCheck};

#[tokio::test]
async fn the_first_claim_wins() {
    let archive = Archive::in_memory().await.unwrap();
    assert!(!archive.password_claimed().await.unwrap());
    let check = archive.verify_password("hunter2").await.unwrap();
    assert_eq!(check, PasswordCheck::Unclaimed);

    assert!(!archive.password_claimed().await.unwrap());
    assert!(archive.claim_password("hunter2").await.unwrap());
    assert!(archive.password_claimed().await.unwrap());

    assert!(!archive.claim_password("usurper").await.unwrap());
    let check = archive.verify_password("usurper").await.unwrap();
    assert_eq!(check, PasswordCheck::Wrong);

    let check = archive.verify_password("hunter2").await.unwrap();
    assert_eq!(check, PasswordCheck::Correct);
}

#[tokio::test]
async fn changing_the_password_retires_the_old_one() {
    let archive = Archive::in_memory().await.unwrap();
    archive.claim_password("hunter2").await.unwrap();

    archive.change_password("hunter3").await.unwrap();

    let check = archive.verify_password("hunter2").await.unwrap();
    assert_eq!(check, PasswordCheck::Wrong);
    let check = archive.verify_password("hunter3").await.unwrap();
    assert_eq!(check, PasswordCheck::Correct);
}
