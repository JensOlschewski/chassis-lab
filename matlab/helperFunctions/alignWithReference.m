% AlignWithReference updates ebasis based on 'align-by-reference' method
%
% This function checks and updates the 'ebasis' property for 'i' and 'j' parts of
% bushing structures if their alignment method is set to 'align-by-reference'.
% It ensures reference ordering is correct and applies the referenced 'ebasis'.
%
% Inputs:
%   bushStructOut - The current state of the bushing structure array.
%   i             - The index of the current bushing element being processed.
%
% Output:
%   bushStructOut - Updated bushing structure array with 'ebasis' aligned as necessary.

function bushStructOut = alignWithReference(bushStructOut, i)
    parts = {'i', 'j'}; % Parts to check and update
    
    for part = parts
        if strcmp(bushStructOut(i).(part{1}).orientMethod, 'align-by-reference')
            if bushStructOut(i).(part{1}).refNumber > i
                error('Reference needs to be ordered before this alignment method');
            end
            bushStructOut(i).(part{1}).eBasis = bushStructOut(bushStructOut(i).(part{1}).refNumber).(bushStructOut(i).(part{1}).refPart).eBasis;
        end
    end
    
    return;
end
