function isKin = isKinActive(bushPrimStruct, bushSecStruct)
    % isKinActive - Checks if any 'active' field in bushPrimStruct or bushSecStruct
    % is true, and sets isKin accordingly.
    %
    % Inputs:
    %   bushPrimStruct - A struct or an array of structs with the field 'active'.
    %   bushSecStruct - Another struct or an array of structs with the field 'active'.
    %
    % Output:
    %   isKin - A logical value, set to false if any 'active' field is found to be true,
    %           otherwise set to true.

    % Check if any 'active' value in bushPrimStruct or bushSecStruct is true
    isActiveInBushPrim = any(arrayfun(@(x) x.active, bushPrimStruct));
    isActiveInBushSec = any(arrayfun(@(x) x.active, bushSecStruct));

    % Set isKin to false if any 'active' value is true, otherwise to true
    isKin = ~(isActiveInBushPrim || isActiveInBushSec);
end
