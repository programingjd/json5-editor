#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputContentType {
    Name,

    NamePrefix,

    GivenName,

    MiddleName,

    FamilyName,

    NameSuffix,

    Nickname,

    JobTitle,

    OrganizationName,

    Location,

    FullStreetAddress,

    StreetAddressLine1,

    StreetAddressLine2,

    AddressCity,

    AddressState,

    AddressCityAndState,

    Sublocality,

    CountryName,

    PostalCode,

    TelephoneNumber,

    EmailAddress,

    Url,

    CreditCardNumber,

    CreditCardName,

    CreditCardGivenName,

    CreditCardMiddleName,

    CreditCardFamilyName,

    CreditCardSecurityCode,

    CreditCardExpiration,

    CreditCardExpirationMonth,

    CreditCardExpirationYear,

    CreditCardType,

    Username,

    Password,

    NewPassword,

    OneTimeCode,

    ShipmentTrackingNumber,

    FlightNumber,

    DateTime,

    Birthdate,

    BirthdateDay,

    BirthdateMonth,

    BirthdateYear,

    CellularEid,

    CellularImei,
}
